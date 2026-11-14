import Foundation
import Tauri
import UIKit
import UniformTypeIdentifiers
import Photos

@_silgen_name("phi_tkc_render_lifecycle")
private func renderLifecycle(_ active: Int32)

private var lifecycleObservers: [NSObjectProtocol] = []

private func installRenderLifecycle() {
    guard Thread.isMainThread else {
        DispatchQueue.main.async { installRenderLifecycle() }
        return
    }
    guard lifecycleObservers.isEmpty else { return }
    lifecycleObservers.append(NotificationCenter.default.addObserver(
        forName: UIApplication.willResignActiveNotification, object: nil, queue: .main
    ) { _ in renderLifecycle(0) })
    lifecycleObservers.append(NotificationCenter.default.addObserver(
        forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
    ) { _ in renderLifecycle(1) })
    lifecycleObservers.append(NotificationCenter.default.addObserver(
        forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main
    ) { _ in renderLifecycle(0) })
    renderLifecycle(UIApplication.shared.applicationState == .active ? 1 : 0)
}

// A renderer may read the chart long after the document picker closes. Import
// while the Files provider's security scope is active, then return a filesystem
// path (URL.path), never the percent-encoded file:// URL.
final class ChartPickerPlugin: Plugin, UIDocumentPickerDelegate {
    private var pending: Invoke?

    private struct VideoArgs: Decodable { let path: String }
    private struct VideoResult: Encodable {
        let saved: Bool
        let assetId: String?
        let message: String
    }

    // PhotoKit runs changes and completion blocks off the caller's queue. Keep
    // their shared placeholder identifier synchronized rather than capturing
    // a mutable local variable across the two callbacks.
    private final class VideoIdentifier: @unchecked Sendable {
        private let lock = NSLock()
        private var value: String?

        func store(_ identifier: String?) {
            lock.lock()
            value = identifier
            lock.unlock()
        }

        func read() -> String? {
            lock.lock()
            defer { lock.unlock() }
            return value
        }
    }

    private func resolveVideo(_ invoke: Invoke, saved: Bool, assetId: String? = nil,
                              message: String) {
        DispatchQueue.main.async {
            invoke.resolve(VideoResult(saved: saved, assetId: assetId, message: message))
        }
    }

    private func saveAuthorizedVideo(_ invoke: Invoke, url: URL,
                                     authorization: PHAuthorizationStatus) {
        // Authorization completion is not guaranteed to run on the main queue.
        guard Thread.isMainThread else {
            DispatchQueue.main.async {
                self.saveAuthorizedVideo(invoke, url: url, authorization: authorization)
            }
            return
        }
        guard authorization == .authorized || authorization == .limited else {
            resolveVideo(invoke, saved: false,
                         message: "未授予相册添加权限，视频已保留在应用内。可在系统设置中允许 Phi-TKC 添加照片。")
            return
        }
        let identifier = VideoIdentifier()
        PHPhotoLibrary.shared().performChanges({
            identifier.store(PHAssetChangeRequest.creationRequestForAssetFromVideo(atFileURL: url)?
                .placeholderForCreatedAsset?.localIdentifier)
        }) { success, error in
            let assetId = identifier.read()
            let saved = success && assetId.map { !$0.isEmpty } == true
            let detail = error?.localizedDescription ?? "相册未返回已创建的视频标识"
            self.resolveVideo(invoke, saved: saved, assetId: saved ? assetId : nil,
                              message: saved ? "已自动保存到相册。" :
                                "无法自动保存到相册，视频已保留在应用内：\(detail)")
        }
    }

    @objc public func saveVideo(_ invoke: Invoke) {
        let args: VideoArgs
        do { args = try invoke.parseArgs(VideoArgs.self) }
        catch {
            let message = "Invalid video path: \(error.localizedDescription)"
            DispatchQueue.main.async { invoke.reject(message) }
            return
        }
        let url = URL(fileURLWithPath: args.path)
        guard FileManager.default.fileExists(atPath: url.path) else {
            resolveVideo(invoke, saved: false, message: "渲染视频文件不可用，未保存到相册。")
            return
        }
        DispatchQueue.main.async {
            let authorization = PHPhotoLibrary.authorizationStatus(for: .addOnly)
            if authorization == .notDetermined {
                PHPhotoLibrary.requestAuthorization(for: .addOnly) { authorization in
                    self.saveAuthorizedVideo(invoke, url: url, authorization: authorization)
                }
            } else {
                self.saveAuthorizedVideo(invoke, url: url, authorization: authorization)
            }
        }
    }

    @objc public func pick(_ invoke: Invoke) {
        presentPicker(invoke, contentTypes: [.item])
    }

    @objc public func pickFolder(_ invoke: Invoke) {
        presentPicker(invoke, contentTypes: [.folder])
    }

    private func presentPicker(_ invoke: Invoke, contentTypes: [UTType]) {
        DispatchQueue.main.async {
            guard self.pending == nil else {
                invoke.reject("A chart picker is already open")
                return
            }
            self.pending = invoke
            let picker = UIDocumentPickerViewController(
                forOpeningContentTypes: contentTypes, asCopy: false)
            picker.delegate = self
            picker.allowsMultipleSelection = false
            picker.modalPresentationStyle = .fullScreen
            guard let presenter = self.manager.viewController else {
                self.pending = nil
                invoke.reject("The app window is unavailable")
                return
            }
            presenter.present(picker, animated: true)
        }
    }

    public func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        pending?.resolve(["path": NSNull()])
        pending = nil
    }

    public func documentPicker(_ controller: UIDocumentPickerViewController,
                               didPickDocumentsAt urls: [URL]) {
        guard let invoke = pending else { return }
        guard let source = urls.first, source.isFileURL else {
            pending = nil
            invoke.reject("The selected document has no file URL")
            return
        }
        let scoped = source.startAccessingSecurityScopedResource()
        DispatchQueue.global(qos: .userInitiated).async {
            defer { if scoped { source.stopAccessingSecurityScopedResource() } }
            let manager = FileManager.default
            var importRoot: URL?
            do {
                let cache = try manager.url(for: .cachesDirectory, in: .userDomainMask,
                                            appropriateFor: nil, create: true)
                let root = cache.appendingPathComponent("chart-imports", isDirectory: true)
                    .appendingPathComponent(UUID().uuidString, isDirectory: true)
                importRoot = root
                try manager.createDirectory(at: root, withIntermediateDirectories: true)
                let destination = root.appendingPathComponent(source.lastPathComponent)
                var coordinationError: NSError?
                var copyError: Error?
                NSFileCoordinator().coordinate(readingItemAt: source, options: [],
                                               error: &coordinationError) { readable in
                    do {
                        // copyItem preserves the complete directory tree and its
                        // relative chart/music/illustration paths, including Unicode.
                        try manager.copyItem(at: readable, to: destination)
                    } catch { copyError = error }
                }
                if let error = coordinationError { throw error }
                if let error = copyError { throw error }
                DispatchQueue.main.async {
                    self.pending = nil
                    invoke.resolve(["path": destination.path])
                }
            } catch {
                if let root = importRoot { try? manager.removeItem(at: root) }
                let message = "Failed to import chart: \(error.localizedDescription)"
                DispatchQueue.main.async {
                    self.pending = nil
                    invoke.reject(message)
                }
            }
        }
    }
}

@_cdecl("init_plugin_chart_picker")
func initPlugin() -> Plugin {
    installRenderLifecycle()
    return ChartPickerPlugin()
}
