import SwiftUI

/// Nyu with a line that says what to do, for empty lists and failures.
struct EmptyState: View {
    enum Mood: String { case happy, sleepy, sad }

    let title: LocalizedStringKey
    let message: LocalizedStringKey
    var mood: Mood = .happy
    var action: (LocalizedStringKey, String, () -> Void)?

    var body: some View {
        VStack(spacing: 12) {
            Image("Nyu-\(mood.rawValue)")
                .resizable()
                .scaledToFit()
                .frame(width: 112, height: 112)
                .accessibilityHidden(true)
            Text(title)
                .font(UwUFont.style(.title3, weight: .bold))
                .foregroundStyle(Palette.ink)
                .multilineTextAlignment(.center)
            Text(message)
                .font(UwUFont.style(.subheadline))
                .foregroundStyle(Palette.muted)
                .multilineTextAlignment(.center)
            if let action {
                Button(action.0, systemImage: action.1, action: action.2)
                    .font(UwUFont.style(.body, weight: .semibold))
                    .primaryButton()
                    .tint(Palette.pinkSolid)
                    .controlSize(.large)
                    .padding(.top, 4)
            }
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity)
    }
}
