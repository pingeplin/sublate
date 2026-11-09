import Foundation
import Security

/// Whether a bundle is signed by the one party allowed to replace the app.
protocol SignatureCheck: Sendable {
    func check(_ bundle: URL) throws
}

/// Developer ID signed by Sublate's team, for Sublate's bundle identifier, nested code included.
/// The download carries no quarantine flag, so Gatekeeper never asks who signed it: this does.
struct DeveloperIDSignature: SignatureCheck {
    var identifier = "tech.radiw.sublate"
    var teamID = "5Q458ND242"

    var requirement: String {
        """
        anchor apple generic \
        and identifier "\(identifier)" \
        and certificate leaf[subject.OU] = "\(teamID)" \
        and certificate 1[field.1.2.840.113635.100.6.2.6] exists \
        and certificate leaf[field.1.2.840.113635.100.6.1.13] exists
        """
    }

    func check(_ bundle: URL) throws {
        var code: SecStaticCode?
        try succeed(SecStaticCodeCreateWithPath(bundle as CFURL, [], &code))
        var requirement: SecRequirement?
        try succeed(SecRequirementCreateWithString(self.requirement as CFString, [], &requirement))
        guard let code, let requirement else { throw AppInstallError.untrusted("no signature") }
        let flags = SecCSFlags(rawValue: kSecCSCheckAllArchitectures | kSecCSCheckNestedCode | kSecCSStrictValidate)
        try succeed(SecStaticCodeCheckValidityWithErrors(code, flags, requirement, nil))
    }

    private func succeed(_ status: OSStatus) throws {
        guard status != errSecSuccess else { return }
        let reason = SecCopyErrorMessageString(status, nil) as String? ?? "error \(status)"
        throw AppInstallError.untrusted(reason)
    }
}

/// Accepts a staged bundle only when it is trusted and is the version that was offered.
struct BundleVerifier: Sendable {
    let signature: any SignatureCheck

    func verify(_ bundle: URL, version: String) throws {
        try signature.check(bundle)
        let found = Self.version(of: bundle)
        guard found == version else { throw AppInstallError.wrongVersion(expected: version, found: found) }
    }

    /// Read from the file, since `Bundle` caches what it reads per path.
    private static func version(of bundle: URL) -> String? {
        let info = NSDictionary(contentsOf: bundle.appending(path: "Contents/Info.plist"))
        return info?["CFBundleShortVersionString"] as? String
    }
}
