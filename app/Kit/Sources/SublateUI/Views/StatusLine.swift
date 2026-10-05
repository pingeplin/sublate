import SwiftUI

struct StatusLine: View {
    let status: Status
    let isBusy: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            if isBusy {
                ProgressView().controlSize(.small)
            }
            switch status {
            case .idle:
                EmptyView()
            case .info(let message):
                Text(message)
            case .failure(let message):
                Text(message)
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
