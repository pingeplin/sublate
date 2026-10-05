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

extension ProgressChannel: TranslationProgressListener where Value == SubtitleProgress {
    func onTranslationProgress(done: UInt64, total: UInt64) {
        send(.translating(done: Int(done), total: Int(total)))
    }
}
