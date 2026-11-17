import AVFoundation
import Foundation

#if canImport(FoundationModels)
import Speech
#endif

// Events of a dictation session, delivered through `WritOnDeviceCallback`.
// Exactly one of failed, stopped and finished ends every session, and nothing
// follows it.
private let dictationPartial: Int32 = 0
private let dictationFinal: Int32 = 1
private let dictationFailed: Int32 = 2
private let dictationStopped: Int32 = 3
private let dictationFinished: Int32 = 4
private let dictationReady: Int32 = 5
private let dictationPreparing: Int32 = 6

// Failure codes, read by `writ_core::dictation::on_device::error_from_bridge`.
private let dictationUnsupported: Int32 = 1
private let dictationLanguageUnsupported: Int32 = 2
private let dictationAssetsFailed: Int32 = 3
private let dictationNoAudioFormat: Int32 = 21
private let dictationAnalyzerFailed: Int32 = 22

/// The format every caller feeds: 16 kHz mono 16-bit samples.
private let dictationSampleRate: Double = 16_000

/// One dictation session. Rust holds it until it releases it, and may stop it
/// at any time before then.
final class WritDictationSession: @unchecked Sendable {
    private let lock = NSLock()
    private let callback: WritOnDeviceCallback
    private let context: UInt
    private var ended = false
    private var task: Task<Void, Never>?
    private var input: WritDictationInput?

    init(context: UnsafeMutableRawPointer?, callback: WritOnDeviceCallback) {
        self.context = UInt(bitPattern: context)
        self.callback = callback
    }

    /// Delivers one event, unless the session has already ended. The ending
    /// event is delivered exactly once.
    func emit(_ kind: Int32, _ code: Int32, _ text: String? = nil) {
        lock.lock()
        defer { lock.unlock() }
        guard !ended else { return }
        if kind == dictationFailed || kind == dictationStopped || kind == dictationFinished {
            ended = true
        }
        let pointer = UnsafeMutableRawPointer(bitPattern: context)
        guard let text else {
            callback(pointer, kind, code, nil)
            return
        }
        text.withCString { callback(pointer, kind, code, $0) }
    }

    func attach(_ task: Task<Void, Never>) {
        lock.lock()
        defer { lock.unlock() }
        if ended {
            task.cancel()
        }
        self.task = task
    }

    /// Takes the running input, or refuses it when the session already ended.
    func attach(_ input: WritDictationInput) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard !ended else { return false }
        self.input = input
        return true
    }

    func feed(_ samples: UnsafePointer<Int16>, _ count: Int) {
        lock.lock()
        let input = self.input
        lock.unlock()
        input?.feed(samples, count)
    }

    func finish() {
        lock.lock()
        let input = self.input
        lock.unlock()
        input?.finish()
    }

    func stop() {
        lock.lock()
        let task = self.task
        let input = self.input
        self.input = nil
        lock.unlock()
        task?.cancel()
        input?.cancel()
        emit(dictationStopped, 0)
    }
}

/// The input side of a running analyzer. Only a macOS 26 session creates one.
class WritDictationInput: @unchecked Sendable {
    func feed(_ samples: UnsafePointer<Int16>, _ count: Int) {}
    func finish() {}
    func cancel() {}
}

/// Whether the microphone may be used, as a code: 0 authorized, 1 not asked
/// yet, 2 denied, 3 restricted by policy.
@_cdecl("writ_dictation_mic_status")
public func writDictationMicStatus() -> Int32 {
    switch AVCaptureDevice.authorizationStatus(for: .audio) {
    case .authorized:
        return 0
    case .notDetermined:
        return 1
    case .denied:
        return 2
    case .restricted:
        return 3
    @unknown default:
        return 2
    }
}

/// Asks for the microphone. `callback` receives kind 1 with code 1 when it was
/// granted and 0 when it was not. A process without a microphone usage string
/// is answered "not granted" without asking, since the system ends a process
/// that asks without one.
@_cdecl("writ_dictation_mic_request")
public func writDictationMicRequest(
    _ context: UnsafeMutableRawPointer?,
    _ callback: WritOnDeviceCallback
) {
    guard Bundle.main.object(forInfoDictionaryKey: "NSMicrophoneUsageDescription") != nil else {
        callback(context, 1, 0, nil)
        return
    }
    let address = UInt(bitPattern: context)
    AVCaptureDevice.requestAccess(for: .audio) { granted in
        callback(UnsafeMutableRawPointer(bitPattern: address), 1, granted ? 1 : 0, nil)
    }
}

/// Lists the locales on-device dictation supports: kind 0 with each BCP 47
/// identifier, then kind 1. Before macOS 26, kind 2 with code 1 alone.
@_cdecl("writ_dictation_supported_locales")
public func writDictationSupportedLocales(
    _ context: UnsafeMutableRawPointer?,
    _ callback: WritOnDeviceCallback
) {
    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        let address = UInt(bitPattern: context)
        Task.detached {
            let pointer = UnsafeMutableRawPointer(bitPattern: address)
            for locale in await DictationTranscriber.supportedLocales {
                locale.identifier(.bcp47).withCString { callback(pointer, 0, 0, $0) }
            }
            callback(pointer, 1, 0, nil)
        }
        return
    }
    #endif
    callback(context, dictationFailed, dictationUnsupported, nil)
}

/// The person's first preferred language as a locale identifier, with the
/// system region added when the language names none. Free the result with
/// `writ_dictation_free_string`.
@_cdecl("writ_dictation_system_locale")
public func writDictationSystemLocale() -> UnsafeMutablePointer<CChar>? {
    let current = Locale.current
    let first = Locale.preferredLanguages.first ?? current.identifier
    let region: String?
    let ownRegion: String?
    if #available(macOS 13, *) {
        region = current.region?.identifier
        ownRegion = Locale(identifier: first).region?.identifier
    } else {
        region = current.regionCode
        ownRegion = Locale(identifier: first).regionCode
    }
    guard ownRegion == nil, let region else {
        return strdup(first)
    }
    return strdup("\(first)-\(region)")
}

/// Frees a string the bridge returned.
@_cdecl("writ_dictation_free_string")
public func writDictationFreeString(_ pointer: UnsafeMutablePointer<CChar>?) {
    free(pointer)
}

/// Starts on-device dictation in `locale` and returns its handle. Kind 6
/// reports a speech model download (`code` the percent, or -1 while unknown),
/// kind 5 says samples may be fed, kind 0 carries the words heard so far and
/// kind 1 a finished stretch of text.
@_cdecl("writ_dictation_start")
public func writDictationStart(
    _ locale: UnsafePointer<CChar>,
    _ context: UnsafeMutableRawPointer?,
    _ callback: WritOnDeviceCallback
) -> UnsafeMutableRawPointer {
    let session = WritDictationSession(context: context, callback: callback)
    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        let identifier = String(cString: locale)
        session.attach(Task.detached { await runDictation(session, identifier) })
        return Unmanaged.passRetained(session).toOpaque()
    }
    #endif
    session.emit(dictationFailed, dictationUnsupported)
    return Unmanaged.passRetained(session).toOpaque()
}

/// Feeds `count` 16 kHz mono samples. Samples fed before the ready event are
/// dropped.
@_cdecl("writ_dictation_feed")
public func writDictationFeed(
    _ handle: UnsafeMutableRawPointer,
    _ samples: UnsafePointer<Int16>,
    _ count: Int
) {
    Unmanaged<WritDictationSession>.fromOpaque(handle).takeUnretainedValue().feed(samples, count)
}

/// Ends the input. The text still pending arrives, then the finished event.
@_cdecl("writ_dictation_finish")
public func writDictationFinish(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<WritDictationSession>.fromOpaque(handle).takeUnretainedValue().finish()
}

/// Stops a session at once. It ends with the stopped event, or with whichever
/// ending it had already reached.
@_cdecl("writ_dictation_stop")
public func writDictationStop(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<WritDictationSession>.fromOpaque(handle).takeUnretainedValue().stop()
}

/// Releases a handle `writ_dictation_start` returned.
@_cdecl("writ_dictation_release")
public func writDictationRelease(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<WritDictationSession>.fromOpaque(handle).release()
}

/// A download's progress as a whole percent from 0 to 100, or -1 when its
/// size or progress is unknown.
@_cdecl("writ_dictation_download_percent")
public func writDictationDownloadPercent(_ fraction: Double, _ total: Int64) -> Int32 {
    guard total > 0, fraction.isFinite else { return -1 }
    return Int32(min(max(fraction, 0), 1) * 100)
}

#if canImport(FoundationModels)
@available(macOS 26.0, *)
private final class AnalyzerInputFeed: WritDictationInput, @unchecked Sendable {
    private let lock = NSLock()
    private let continuation: AsyncStream<AnalyzerInput>.Continuation
    private let analyzer: SpeechAnalyzer
    private let source: AVAudioFormat
    private let target: AVAudioFormat
    private let converter: AVAudioConverter?

    init(
        continuation: AsyncStream<AnalyzerInput>.Continuation,
        analyzer: SpeechAnalyzer,
        source: AVAudioFormat,
        target: AVAudioFormat,
        converter: AVAudioConverter?
    ) {
        self.continuation = continuation
        self.analyzer = analyzer
        self.source = source
        self.target = target
        self.converter = converter
    }

    override func feed(_ samples: UnsafePointer<Int16>, _ count: Int) {
        guard count > 0,
            let buffer = AVAudioPCMBuffer(pcmFormat: source, frameCapacity: AVAudioFrameCount(count)),
            let channel = buffer.int16ChannelData
        else { return }
        channel[0].update(from: samples, count: count)
        buffer.frameLength = AVAudioFrameCount(count)
        lock.lock()
        defer { lock.unlock() }
        guard let converter else {
            continuation.yield(AnalyzerInput(buffer: buffer))
            return
        }
        let ratio = target.sampleRate / source.sampleRate
        let capacity = AVAudioFrameCount((Double(count) * ratio).rounded(.up)) + 64
        guard let converted = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else {
            return
        }
        var supplied = false
        var error: NSError?
        let status = converter.convert(to: converted, error: &error) { _, inputStatus in
            if supplied {
                inputStatus.pointee = .noDataNow
                return nil
            }
            supplied = true
            inputStatus.pointee = .haveData
            return buffer
        }
        if status != .error, converted.frameLength > 0 {
            continuation.yield(AnalyzerInput(buffer: converted))
        }
    }

    override func finish() {
        lock.lock()
        defer { lock.unlock() }
        if let converter {
            flush(converter)
        }
        continuation.finish()
    }

    /// Sends the samples a sample-rate converter still holds once the input
    /// has ended.
    private func flush(_ converter: AVAudioConverter) {
        for _ in 0..<8 {
            guard let tail = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: 4096) else {
                return
            }
            var error: NSError?
            let status = converter.convert(to: tail, error: &error) { _, inputStatus in
                inputStatus.pointee = .endOfStream
                return nil
            }
            if status != .error, tail.frameLength > 0 {
                continuation.yield(AnalyzerInput(buffer: tail))
            }
            if status != .haveData {
                return
            }
        }
    }

    override func cancel() {
        continuation.finish()
        let analyzer = self.analyzer
        Task.detached { await analyzer.cancelAndFinishNow() }
    }
}

/// Reports the download of `request` as kind 6 events until cancelled.
@available(macOS 26.0, *)
private func reportProgress(_ progress: Progress, _ session: WritDictationSession) async {
    var last: Int32 = -2
    while !Task.isCancelled {
        let percent = writDictationDownloadPercent(
            progress.fractionCompleted, progress.totalUnitCount)
        if percent != last {
            last = percent
            session.emit(dictationPreparing, percent)
        }
        try? await Task.sleep(nanoseconds: 250_000_000)
    }
}

/// Runs one session to its end: the locale check, the speech model download,
/// the analyzer, and every result until the input ends.
@available(macOS 26.0, *)
private func runDictation(_ session: WritDictationSession, _ identifier: String) async {
    let wanted = Locale(identifier: identifier).identifier(.bcp47)
    let supported = await DictationTranscriber.supportedLocales
    guard let locale = supported.first(where: { $0.identifier(.bcp47) == wanted }) else {
        session.emit(dictationFailed, dictationLanguageUnsupported)
        return
    }
    let transcriber = DictationTranscriber(
        locale: locale,
        contentHints: [],
        transcriptionOptions: [.punctuation],
        reportingOptions: [.volatileResults],
        attributeOptions: []
    )
    do {
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
            let reporter = Task { await reportProgress(request.progress, session) }
            defer { reporter.cancel() }
            try await request.downloadAndInstall()
        }
    } catch {
        session.emit(dictationFailed, dictationAssetsFailed)
        return
    }
    if Task.isCancelled { return }

    guard
        let source = AVAudioFormat(
            commonFormat: .pcmFormatInt16, sampleRate: dictationSampleRate, channels: 1,
            interleaved: false),
        let target = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [transcriber])
    else {
        session.emit(dictationFailed, dictationNoAudioFormat)
        return
    }
    var converter: AVAudioConverter?
    if !target.isEqual(source) {
        guard let made = AVAudioConverter(from: source, to: target) else {
            session.emit(dictationFailed, dictationNoAudioFormat)
            return
        }
        converter = made
    }

    let analyzer = SpeechAnalyzer(modules: [transcriber])
    let (stream, continuation) = AsyncStream<AnalyzerInput>.makeStream()
    let results = Task { () -> Bool in
        do {
            for try await result in transcriber.results {
                let text = String(result.text.characters)
                session.emit(result.isFinal ? dictationFinal : dictationPartial, 0, text)
            }
            return true
        } catch {
            return false
        }
    }
    do {
        try await analyzer.start(inputSequence: stream)
    } catch {
        continuation.finish()
        results.cancel()
        session.emit(dictationFailed, dictationAnalyzerFailed)
        return
    }
    let input = AnalyzerInputFeed(
        continuation: continuation, analyzer: analyzer, source: source, target: target,
        converter: converter)
    guard session.attach(input) else {
        input.cancel()
        results.cancel()
        return
    }
    session.emit(dictationReady, 0)

    do {
        try await analyzer.finalizeAndFinishThroughEndOfInput()
    } catch {
        results.cancel()
        session.emit(dictationFailed, dictationAnalyzerFailed)
        return
    }
    if await results.value {
        session.emit(dictationFinished, 0)
    } else {
        session.emit(dictationFailed, dictationAnalyzerFailed)
    }
}
#endif
