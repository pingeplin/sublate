import Foundation
import Testing

@testable import SublateUI

struct BundleVerifierTests {
    let temp: TempFolder

    init() throws {
        temp = try TempFolder()
    }

    enum Untrusted: CaseIterable {
        case unsigned, signedAdHoc, signedByAnotherParty
    }

    @Test(arguments: Untrusted.allCases)
    func aBundleNotSignedBySublatesDeveloperIsRejected(_ kind: Untrusted) throws {
        let bundle = switch kind {
        case .unsigned: try FakeBundle.make(at: temp.url.appending(path: "Sublate.app"), version: "0.2.0", marker: "x")
        case .signedAdHoc: try FakeBundle.makeAdHocSigned(at: temp.url.appending(path: "Sublate.app"), version: "0.2.0")
        case .signedByAnotherParty: URL(filePath: "/System/Applications/Calculator.app")
        }

        let error = #expect(throws: AppInstallError.self) { try DeveloperIDSignature().check(bundle) }

        guard case .untrusted = error else {
            Issue.record("\(String(describing: error)) is not a rejection of the signature")
            return
        }
    }

    @Test func aTrustedBundleOfAnotherVersionIsRejected() throws {
        let bundle = try FakeBundle.make(at: temp.url.appending(path: "Sublate.app"), version: "0.1.9", marker: "x")
        let verifier = BundleVerifier(signature: AcceptsAnySignature())

        let error = #expect(throws: AppInstallError.self) { try verifier.verify(bundle, version: "0.2.0") }

        #expect(error == .wrongVersion(expected: "0.2.0", found: "0.1.9"))
        let message = error?.localizedDescription ?? ""
        #expect(message.contains("0.1.9") && message.contains("0.2.0"))
    }

    @Test func aTrustedBundleOfTheOfferedVersionIsAccepted() throws {
        let bundle = try FakeBundle.make(at: temp.url.appending(path: "Sublate.app"), version: "0.2.0", marker: "x")

        try BundleVerifier(signature: AcceptsAnySignature()).verify(bundle, version: "0.2.0")
    }
}

/// Needs a Sublate build signed with the Developer ID: `make build` first. Run with `LIVE=1 swift test`.
@Suite(.enabled(if: ProcessInfo.processInfo.environment["LIVE"] != nil))
struct LiveBundleVerifierTests {
    static let build = URL(filePath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appending(path: "build/Build/Products/Release/Sublate.app")

    @Test func theReleaseBuildIsTrusted() throws {
        try DeveloperIDSignature().check(Self.build)
    }

    @Test func theSameBuildIsRejectedForAnotherTeamOrIdentifier() {
        #expect(throws: AppInstallError.self) { try DeveloperIDSignature(teamID: "AAAAAAAAAA").check(Self.build) }
        #expect(throws: AppInstallError.self) { try DeveloperIDSignature(identifier: "com.example.other").check(Self.build) }
    }
}
