import UIKit
import AVFoundation
import PhiTKCNativeBridge

// UIKit owns the full-screen presentation. Its aspect-fit rectangle is shared
// by drawing and touch conversion, including after a device rotation.
private final class PreviewImageView: UIView {
    let imageView = UIImageView()
    let session: UInt64
    let imageSize: CGSize
    init(session: UInt64, width: Int, height: Int) {
        self.session = session
        imageSize = CGSize(width: width, height: height)
        super.init(frame: .zero)
        backgroundColor = .black
        isMultipleTouchEnabled = true
        imageView.contentMode = .scaleAspectFit
        imageView.isUserInteractionEnabled = false
        addSubview(imageView)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    override func layoutSubviews() {
        super.layoutSubviews()
        imageView.frame = bounds
    }
    private var imageRect: CGRect {
        let scale = min(bounds.width / imageSize.width, bounds.height / imageSize.height)
        let size = CGSize(width: imageSize.width * scale, height: imageSize.height * scale)
        return CGRect(x: (bounds.width-size.width)/2, y: (bounds.height-size.height)/2,
                      width: size.width, height: size.height)
    }
    private func send(_ touches: Set<UITouch>, phase: Int32, event: UIEvent?) {
        let rect = imageRect
        guard rect.width > 0 && rect.height > 0 else { return }
        for touch in touches {
            let contact = UInt64(UInt(bitPattern: Unmanaged.passUnretained(touch).toOpaque()))
            let samples = phase == 1 ? (event?.coalescedTouches(for: touch) ?? [touch]) : [touch]
            for sample in samples {
                let point = sample.location(in: self)
                phi_tkc_preview_touch(session, contact, phase,
                    Float((point.x-rect.minX)/rect.width*imageSize.width),
                    Float((point.y-rect.minY)/rect.height*imageSize.height), sample.timestamp)
            }
        }
    }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, phase: 0, event: event) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, phase: 1, event: event) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, phase: 2, event: event) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, phase: 3, event: event) }
}

final class PreviewViewController: UIViewController {
    private let session: UInt64
    private let width: Int
    private let height: Int
    private let showFps: Bool
    private let previousOrientation: UIInterfaceOrientation
    private let completed: () -> Void
    private var pixels: Data
    private var link: CADisplayLink?
    private var observers: [NSObjectProtocol] = []
    private var closed = false
    private var finished = false
    private var awaitingExternalTransition = false
    private var loading: UILabel?
    private var fpsLabel: UILabel?
    private var maximumFramesPerSecond = 0
    private weak var previewScene: UIWindowScene?
    private weak var presenter: UIViewController?

    init(session: UInt64, width: Int, height: Int, showFps: Bool,
         previousOrientation: UIInterfaceOrientation, completed: @escaping () -> Void) {
        self.session = session
        self.width = width
        self.height = height
        self.showFps = showFps
        self.previousOrientation = previousOrientation
        self.completed = completed
        pixels = Data(count: width * height * 4)
        super.init(nibName: nil, bundle: nil)
        modalPresentationStyle = .fullScreen
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    override var prefersStatusBarHidden: Bool { true }
    override var prefersHomeIndicatorAutoHidden: Bool { true }
    override var supportedInterfaceOrientations: UIInterfaceOrientationMask {
        width >= height ? .landscape : .portrait
    }
    override var preferredInterfaceOrientationForPresentation: UIInterfaceOrientation {
        width >= height ? .landscapeRight : .portrait
    }
    override var shouldAutorotate: Bool { true }

    override func loadView() {
        let image = PreviewImageView(session: session, width: width, height: height)
        view = image
        let loading = UILabel()
        loading.text = "Loading…"
        loading.textColor = .white
        loading.translatesAutoresizingMaskIntoConstraints = false
        image.addSubview(loading)
        NSLayoutConstraint.activate([loading.centerXAnchor.constraint(equalTo: image.centerXAnchor),
                                     loading.centerYAnchor.constraint(equalTo: image.centerYAnchor)])
        self.loading = loading
        if showFps {
            let label = UILabel()
            label.text = "0.0 FPS"
            label.font = .monospacedDigitSystemFont(ofSize: 13, weight: .medium)
            label.textColor = .white
            label.backgroundColor = UIColor.black.withAlphaComponent(0.45)
            label.isUserInteractionEnabled = false
            label.translatesAutoresizingMaskIntoConstraints = false
            image.addSubview(label)
            NSLayoutConstraint.activate([
                label.trailingAnchor.constraint(equalTo: image.safeAreaLayoutGuide.trailingAnchor, constant: -12),
                label.bottomAnchor.constraint(equalTo: image.safeAreaLayoutGuide.bottomAnchor, constant: -8)
            ])
            fpsLabel = label
        }
    }
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        UIApplication.shared.isIdleTimerDisabled = true
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .default)
        try? AVAudioSession.sharedInstance().setActive(true)
        previewScene = view.window?.windowScene
        presenter = presentingViewController
        if #available(iOS 16.0, *) {
            setNeedsUpdateOfSupportedInterfaceOrientations()
            previewScene?.requestGeometryUpdate(.iOS(interfaceOrientations: supportedInterfaceOrientations))
        }
        let link = CADisplayLink(target: self, selector: #selector(frame(_:)))
        self.link = link
        configureFrameRate()
        link.add(to: .main, forMode: .common)
        observers.append(NotificationCenter.default.addObserver(
            forName: UIApplication.willResignActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.setActive(false) })
        observers.append(NotificationCenter.default.addObserver(
            forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.setActive(true) })
        setActive(UIApplication.shared.applicationState == .active)
    }
    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        configureFrameRate()
    }
    private func configureFrameRate() {
        guard let link = link else { return }
        let maximum = min(120, view.window?.screen.maximumFramesPerSecond ?? 60)
        guard maximum != maximumFramesPerSecond else { return }
        maximumFramesPerSecond = maximum
        if #available(iOS 15.0, *) {
            link.preferredFrameRateRange = CAFrameRateRange(minimum: Float(min(60, maximum)),
                maximum: Float(maximum), preferred: Float(maximum))
        } else {
            link.preferredFramesPerSecond = maximum
        }
    }
    private func setActive(_ active: Bool) {
        if active { configureFrameRate() }
        phi_tkc_preview_active(session, active ? 1 : 0)
        link?.isPaused = !active
    }
    @objc private func frame(_ link: CADisplayLink) {
        // The display link presents the latest completed frame. Rust renders
        // independently at the FPS requested in the preview settings.
        if let label = fpsLabel {
            let text = String(format: "%.1f FPS", phi_tkc_preview_fps(session))
            if text != label.text { label.text = text }
        }
        let status = pixels.withUnsafeMutableBytes { bytes in
            phi_tkc_preview_read_frame(session, bytes.bindMemory(to: UInt8.self).baseAddress,
                                       numericCast(bytes.count))
        }
        if status < 0 { close(); return }
        guard status == 1,
              let provider = CGDataProvider(data: pixels as CFData),
              let cgImage = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                  bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue)
                    .union(.byteOrder32Big), provider: provider, decode: nil,
                  shouldInterpolate: true, intent: .defaultIntent) else { return }
        loading?.removeFromSuperview()
        loading = nil
        (view as? PreviewImageView)?.imageView.image = UIImage(cgImage: cgImage)
    }
    @objc private func close() {
        guard !closed else { return }
        closed = true
        tearDown()
        dismiss(animated: true) { self.finish() }
    }
    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        guard !closed, let transition = transitionCoordinator else { return }
        awaitingExternalTransition = true
        // Register before the transition finishes. The return value only says
        // whether an animation block was queued, not whether completion runs.
        transition.animate(alongsideTransition: nil) { context in
            self.awaitingExternalTransition = false
            guard !context.isCancelled else { return }
            self.closed = true
            self.tearDown()
            self.finish()
        }
    }
    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        tearDown()
        // Our own dismissal finishes in its completion block, after UIKit has
        // removed this controller from the orientation decision. An external
        // dismissal waits for its transition before restoring the presenter.
        guard !closed else { return }
        closed = true
        if !awaitingExternalTransition {
            DispatchQueue.main.async { self.finish() }
        }
    }
    private func tearDown() {
        link?.invalidate()
        link = nil
        observers.forEach { NotificationCenter.default.removeObserver($0) }
        observers.removeAll()
        UIApplication.shared.isIdleTimerDisabled = false
        phi_tkc_preview_close(session)
    }
    private func finish() {
        guard !finished else { return }
        finished = true
        if #available(iOS 16.0, *) {
            let orientation: UIInterfaceOrientationMask
            switch previousOrientation {
            case .portrait: orientation = .portrait
            case .portraitUpsideDown: orientation = .portraitUpsideDown
            case .landscapeLeft: orientation = .landscapeLeft
            case .landscapeRight: orientation = .landscapeRight
            default: orientation = []
            }
            if !orientation.isEmpty {
                presenter?.setNeedsUpdateOfSupportedInterfaceOrientations()
                previewScene?.requestGeometryUpdate(.iOS(interfaceOrientations: orientation))
            }
        }
        completed()
    }
}
