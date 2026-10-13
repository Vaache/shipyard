import SwiftUI

struct SplashView: View {
    @State private var appeared = false
    @State private var breathing = false

    var body: some View {
        ZStack {
            Color(nsColor: Theme.isBrand ? .syWindow : .windowBackgroundColor)
            if Theme.isBrand {
                RadialGradient(colors: [Color.syViolet.opacity(0.20), .clear], center: UnitPoint(x: 0.36, y: 0.32), startRadius: 0, endRadius: 440)
                RadialGradient(colors: [Color.sySky.opacity(0.14), .clear], center: UnitPoint(x: 0.66, y: 0.28), startRadius: 0, endRadius: 440)
            }
            VStack(spacing: 20) {
                Image(nsImage: NSApp.applicationIconImage)
                    .resizable()
                    .interpolation(.high)
                    .frame(width: 136, height: 136)
                    .shadow(color: .black.opacity(0.32), radius: 26, y: 16)
                    .scaleEffect(appeared ? (breathing ? 1.025 : 1) : 0.84)
                VStack(spacing: 6) {
                    Text("Shipyard")
                        .font(.system(size: 32, weight: .bold))
                        .foregroundStyle(Color.syText)
                    Text("Pull requests, without the browser")
                        .foregroundStyle(Color.syMuted)
                }
                .opacity(appeared ? 1 : 0)
                .offset(y: appeared || Motion.reduced ? 0 : 10)
                ProgressView()
                    .controlSize(.small)
                    .opacity(appeared ? 0.7 : 0)
            }
            .opacity(appeared ? 1 : 0)
        }
        .ignoresSafeArea()
        .onAppear {
            withAnimation(Motion.reduced ? nil : .spring(response: 0.6, dampingFraction: 0.72)) { appeared = true }
            guard !Motion.reduced else { return }
            withAnimation(.easeInOut(duration: 1.8).repeatForever(autoreverses: true).delay(0.6)) { breathing = true }
        }
    }
}
