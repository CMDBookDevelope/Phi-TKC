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
    private let completed: () -> Void
    private var pixels: Data
    private var link: CADisplayLink?
    private var observers: [NSObjectProtocol] = []
    private var closed = false
    private var finished = false
    private var loading: UILabel?

    init(session: UInt64, width: Int, height: Int, completed: @escaping () -> Void) {
        self.session = session
        self.width = width
        self.height = height
        self.completed = completed
        pixels = Data(count: width * height * 4)
        super.init(nibName: nil, bundle: nil)
        modalPresentationStyle = .fullScreen
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    override var prefersStatusBarHidden: Bool { true }
    override var supportedInterfaceOrientations: UIInterfaceOrientationMask { .allButUpsideDown }
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
        let close = UIButton(type: .system)
        close.setTitle("×", for: .normal)
        close.titleLabel?.font = .systemFont(ofSize: 32)
        close.setTitleColor(.white, for: .normal)
        close.backgroundColor = UIColor.black.withAlphaComponent(0.5)
        close.accessibilityLabel = "Close preview"
        close.translatesAutoresizingMaskIntoConstraints = false
        close.addTarget(self, action: #selector(PreviewViewController.close), for: .touchUpInside)
        image.addSubview(close)
        NSLayoutConstraint.activate([close.widthAnchor.constraint(equalToConstant: 48),
                                     close.heightAnchor.constraint(equalToConstant: 48),
                                     close.topAnchor.constraint(equalTo: image.safeAreaLayoutGuide.topAnchor),
                                     close.trailingAnchor.constraint(equalTo: image.safeAreaLayoutGuide.trailingAnchor)])
    }
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        UIApplication.shared.isIdleTimerDisabled = true
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .default)
        try? AVAudioSession.sharedInstance().setActive(true)
        let link = CADisplayLink(target: self, selector: #selector(frame))
        link.preferredFramesPerSecond = 60
        link.add(to: .main, forMode: .common)
        self.link = link
        observers.append(NotificationCenter.default.addObserver(
            forName: UIApplication.willResignActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.setActive(false) })
        observers.append(NotificationCenter.default.addObserver(
            forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.setActive(true) })
        setActive(UIApplication.shared.applicationState == .active)
    }
    private func setActive(_ active: Bool) {
        phi_tkc_preview_active(session, active ? 1 : 0)
        link?.isPaused = !active
    }
    @objc private func frame() {
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
    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        tearDown()
        finish()
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
        completed()
    }
}
