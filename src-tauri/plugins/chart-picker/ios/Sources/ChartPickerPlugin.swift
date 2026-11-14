import Foundation
import Tauri
import UIKit
import UniformTypeIdentifiers

// A renderer may read the chart long after the document picker closes. Import
// while the Files provider's security scope is active, then return a filesystem
// path (URL.path), never the percent-encoded file:// URL.
final class ChartPickerPlugin: Plugin, UIDocumentPickerDelegate {
    private var pending: Invoke?

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
    ChartPickerPlugin()
}
