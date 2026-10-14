import Foundation

#if canImport(FoundationModels)
import FoundationModels
#endif

/// Receives each event of a stream. `kind` is 0 for the reply so far (`text`),
/// 1 when the reply is complete, 2 when it failed (`code`), 3 when it was
/// stopped. Exactly one of 1, 2 and 3 ends every stream, and nothing follows it.
public typealias WritOnDeviceCallback = @convention(c) (
    UnsafeMutableRawPointer?, Int32, Int32, UnsafePointer<CChar>?
) -> Void

private let eventSnapshot: Int32 = 0
private let eventDone: Int32 = 1
private let eventFailed: Int32 = 2
private let eventStopped: Int32 = 3

/// Availability codes, shared with the failure codes a stream ends with.
private let codeAvailable: Int32 = 0
private let codeUnsupported: Int32 = 1
private let codeDeviceNotEligible: Int32 = 2
private let codeAppleIntelligenceOff: Int32 = 3
private let codeModelNotReady: Int32 = 4
private let codeUnavailableOther: Int32 = 5
private let codeOtherFailure: Int32 = 20

/// One running request. Rust holds it until it releases it, and may stop it
/// at any time before then.
final class WritOnDeviceStream: @unchecked Sendable {
    private let lock = NSLock()
    private var task: Task<Void, Never>?
    private var stopped = false

    func attach(_ task: Task<Void, Never>) {
        lock.lock()
        defer { lock.unlock() }
        if stopped {
            task.cancel()
        }
        self.task = task
    }

    func stop() {
        lock.lock()
        defer { lock.unlock() }
        stopped = true
        task?.cancel()
    }
}

/// The running system's major version.
@_cdecl("writ_on_device_os_major")
public func writOnDeviceOsMajor() -> Int32 {
    Int32(ProcessInfo.processInfo.operatingSystemVersion.majorVersion)
}

/// Whether the on-device model can answer now, as an availability code.
@_cdecl("writ_on_device_availability")
public func writOnDeviceAvailability() -> Int32 {
    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        return availabilityCode(SystemLanguageModel.default.availability)
    }
    #endif
    return codeUnsupported
}

/// Starts a rewrite and returns its handle. `callback` receives every event
/// with `context`, on a thread of the system's choosing.
@_cdecl("writ_on_device_stream")
public func writOnDeviceStream(
    _ instructions: UnsafePointer<CChar>,
    _ prompt: UnsafePointer<CChar>,
    _ temperature: Double,
    _ context: UnsafeMutableRawPointer?,
    _ callback: WritOnDeviceCallback
) -> UnsafeMutableRawPointer {
    let stream = WritOnDeviceStream()
    let instructionsText = String(cString: instructions)
    let promptText = String(cString: prompt)
    let address = UInt(bitPattern: context)
    let emit: @Sendable (Int32, Int32, String?) -> Void = { kind, code, text in
        let pointer = UnsafeMutableRawPointer(bitPattern: address)
        guard let text else {
            callback(pointer, kind, code, nil)
            return
        }
        text.withCString { callback(pointer, kind, code, $0) }
    }

    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        stream.attach(
            Task.detached {
                await rewrite(instructionsText, promptText, temperature, emit)
            })
        return Unmanaged.passRetained(stream).toOpaque()
    }
    #endif
    emit(eventFailed, codeUnsupported, nil)
    return Unmanaged.passRetained(stream).toOpaque()
}

/// Stops a rewrite. Its stream ends with the stopped event, or with whichever
/// ending it had already reached.
@_cdecl("writ_on_device_stop")
public func writOnDeviceStop(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<WritOnDeviceStream>.fromOpaque(handle).takeUnretainedValue().stop()
}

/// Releases a handle `writ_on_device_stream` returned.
@_cdecl("writ_on_device_release")
public func writOnDeviceRelease(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<WritOnDeviceStream>.fromOpaque(handle).release()
}

#if canImport(FoundationModels)
@available(macOS 26.0, *)
private func availabilityCode(_ availability: SystemLanguageModel.Availability) -> Int32 {
    switch availability {
    case .available:
        return codeAvailable
    case .unavailable(.deviceNotEligible):
        return codeDeviceNotEligible
    case .unavailable(.appleIntelligenceNotEnabled):
        return codeAppleIntelligenceOff
    case .unavailable(.modelNotReady):
        return codeModelNotReady
    case .unavailable:
        return codeUnavailableOther
    }
}

@available(macOS 26.0, *)
private func failureCode(_ error: LanguageModelSession.GenerationError) -> Int32 {
    switch error {
    case .exceededContextWindowSize:
        return 10
    case .assetsUnavailable:
        return 11
    case .guardrailViolation:
        return 12
    case .unsupportedLanguageOrLocale:
        return 13
    case .rateLimited:
        return 14
    case .concurrentRequests:
        return 15
    case .refusal:
        return 16
    case .unsupportedGuide, .decodingFailure:
        return 17
    @unknown default:
        return codeOtherFailure
    }
}

/// Streams one rewrite, ending with exactly one of done, failed and stopped.
@available(macOS 26.0, *)
private func rewrite(
    _ instructions: String,
    _ prompt: String,
    _ temperature: Double,
    _ emit: @Sendable (Int32, Int32, String?) -> Void
) async {
    // Rewriting a person's own text is a content transformation, which the
    // default guardrails decline far more often than the request warrants.
    let model = SystemLanguageModel(guardrails: .permissiveContentTransformations)
    let availability = availabilityCode(model.availability)
    guard availability == codeAvailable else {
        emit(eventFailed, availability, nil)
        return
    }
    let session = LanguageModelSession(model: model, instructions: instructions)
    let options = GenerationOptions(temperature: temperature)
    do {
        for try await snapshot in session.streamResponse(to: prompt, options: options) {
            if Task.isCancelled { break }
            emit(eventSnapshot, 0, snapshot.content)
        }
        emit(Task.isCancelled ? eventStopped : eventDone, 0, nil)
    } catch let error as LanguageModelSession.GenerationError {
        emit(Task.isCancelled ? eventStopped : eventFailed, failureCode(error), nil)
    } catch {
        emit(Task.isCancelled ? eventStopped : eventFailed, codeOtherFailure, nil)
    }
}
#endif
