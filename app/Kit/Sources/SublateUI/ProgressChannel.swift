import SublateCore

/// Carries progress the backend reports off the main thread to a single in-order consumer.
/// Only the latest value matters, so a slow consumer skips stale ones.
final class ProgressChannel<Value: Sendable>: Sendable {
    let values: AsyncStream<Value>
    private let continuation: AsyncStream<Value>.Continuation

    init() {
        (values, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }

    func send(_ value: Value) {
        continuation.yield(value)
    }

    func finish() {
        continuation.finish()
    }
}

extension ProgressChannel: VideoProgressListener where Value == Double {
    func onVideoProgress(percent: Float) {
        send(Double(percent))
    }
}

extension ProgressChannel: AppDownloadListener where Value == Double? {
    /// The fraction downloaded, or `nil` while the size is unknown.
    func onDownloadProgress(done: UInt64, total: UInt64?) {
        send(total.flatMap { $0 == 0 ? nil : Double(done) / Double($0) })
    }
}

/// Applies every progress value the operation reports before handing back its result.
@MainActor
func tracking<Value: Sendable, Output: Sendable>(
    _ apply: (Value) -> Void,
    during operation: @Sendable (ProgressChannel<Value>) async throws -> Output
) async throws -> Output {
    let channel = ProgressChannel<Value>()
    async let output = {
        defer { channel.finish() }
        return try await operation(channel)
    }()
    for await value in channel.values {
        apply(value)
    }
    return try await output
}

extension ProgressChannel: TranslationProgressListener where Value == SubtitleProgress {
    func onTranslationProgress(done: UInt64, total: UInt64) {
        send(.translating(done: Int(done), total: Int(total)))
    }
}
