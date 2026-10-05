import SublateCore
import Foundation

extension Error {
    /// The backend's own wording; anything else falls back to Foundation's description.
    var userMessage: String {
        if case .Failed(let message) = self as? BackendError { return message }
        return localizedDescription
    }
}

extension CredentialState {
    var label: String {
        switch self {
        case .checking: "Checking…"
        case .missing: "No API key yet"
        case .available(let source): source
        }
    }

    /// Shown under the window title.
    var subtitle: String {
        switch self {
        case .checking: ""
        case .missing: "Add an Anthropic API key in Settings (⌘,)"
        case .available(let source): "Claude via \(source)"
        }
    }
}

extension VideoMetadata {
    var reference: VideoRef {
        VideoRef(url: url, id: id, title: title)
    }

    var durationText: String? {
        duration.map { seconds in
            Duration.seconds(seconds)
                .formatted(.time(pattern: seconds < 3600 ? .minuteSecond : .hourMinuteSecond))
        }
    }
}

extension SubtitleTrack {
    var label: String { "\(name) (\(code))" }
}

extension TargetLanguage {
    var label: String { "\(native) (\(code))" }
}

extension TrackKind {
    static let displayOrder: [TrackKind] = [.manual, .auto]

    var title: String {
        switch self {
        case .manual: "Uploaded"
        case .auto: "Auto-generated"
        }
    }
}

extension URL {
    var filePath: String { path(percentEncoded: false) }

    var abbreviatedPath: String { (filePath as NSString).abbreviatingWithTildeInPath }
}
