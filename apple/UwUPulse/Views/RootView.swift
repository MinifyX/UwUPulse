import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        TabView(selection: $model.selectedTab) {
            Tab("Home", systemImage: "heart.fill", value: AppModel.AppTab.home) {
                NavigationStack { HomeView() }
            }
            Tab("Desktops", systemImage: "desktopcomputer", value: AppModel.AppTab.desktops) {
                NavigationStack { DesktopsView() }
            }
            Tab("Chest straps", systemImage: "dot.radiowaves.left.and.right", value: AppModel.AppTab.straps) {
                NavigationStack { StrapsView() }
            }
            Tab("About", systemImage: "info.circle", value: AppModel.AppTab.about) {
                NavigationStack { AboutView() }
            }
        }
        .sheet(isPresented: $model.pairingSheetShown, onDismiss: { model.links.cancelPairing() }) {
            PairingSheet()
        }
        .sheet(item: $model.offerToConfirm) { confirmation in
            ConfirmOfferSheet(confirmation: confirmation)
        }
    }
}

/// Pulsing heart; beats roughly at the current rate.
struct PulsingHeart: View {
    var bpm: Int?
    var size: CGFloat = 120

    var body: some View {
        let heart = Image(systemName: bpm != nil ? "heart.fill" : "heart")
            .font(.system(size: size))
            .foregroundStyle(Palette.pink.gradient)
            .accessibilityHidden(true)
        if let bpm {
            let period = 60.0 / Double(max(40, min(bpm, 200)))
            heart.phaseAnimator([false, true]) { view, beat in
                view.scaleEffect(beat ? 1.12 : 1.0)
            } animation: { beat in
                beat ? .easeOut(duration: period * 0.3) : .easeIn(duration: period * 0.7)
            }
        } else {
            heart
        }
    }
}
