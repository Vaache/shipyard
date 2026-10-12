import SwiftUI
import WebKit

struct HTMLView: NSViewRepresentable {
    let html: String
    var scrollToBottomToken = 0
    var onAction: (String, String) -> Void = { _, _ in }
    var onLink: (URL) -> Void = { NSWorkspace.shared.open($0) }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.userContentController.add(WeakHandler(context.coordinator), name: "app")
        config.preferences.isElementFullscreenEnabled = false
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.uiDelegate = context.coordinator
        view.setValue(false, forKey: "drawsBackground")
        view.allowsMagnification = true
        context.coordinator.render(in: view, html: html, token: scrollToBottomToken)
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {
        context.coordinator.parent = self
        context.coordinator.render(in: view, html: html, token: scrollToBottomToken)
    }

    static func dismantleNSView(_ view: WKWebView, coordinator: Coordinator) {
        view.configuration.userContentController.removeScriptMessageHandler(forName: "app")
    }

    final class WeakHandler: NSObject, WKScriptMessageHandler {
        weak var target: Coordinator?
        init(_ target: Coordinator) { self.target = target }
        func userContentController(_ c: WKUserContentController, didReceive message: WKScriptMessage) {
            target?.userContentController(c, didReceive: message)
        }
    }

    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler {
        var parent: HTMLView
        private var html = ""
        private var token = 0
        private var restoreY: Double?
        private var scrollBottom = false

        init(_ parent: HTMLView) {
            self.parent = parent
            self.token = parent.scrollToBottomToken
        }

        func render(in view: WKWebView, html: String, token: Int) {
            if token != self.token {
                self.token = token
                scrollBottom = true
            }
            guard html != self.html else {
                if scrollBottom {
                    scrollBottom = false
                    view.evaluateJavaScript("window.scrollTo(0, document.body.scrollHeight)")
                }
                return
            }
            let first = self.html.isEmpty
            self.html = html
            if first {
                view.loadHTMLString(html, baseURL: URL(string: "https://github.com/"))
                return
            }
            view.evaluateJavaScript("window.scrollY") { [weak self, weak view] y, _ in
                self?.restoreY = y as? Double
                view?.loadHTMLString(html, baseURL: URL(string: "https://github.com/"))
            }
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            if scrollBottom {
                scrollBottom = false
                webView.evaluateJavaScript("window.scrollTo(0, document.body.scrollHeight)")
            } else if let y = restoreY {
                webView.evaluateJavaScript("window.scrollTo(0, \(y))")
            }
            restoreY = nil
        }

        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            if action.navigationType == .linkActivated, let url = action.request.url {
                decisionHandler(.cancel)
                parent.onLink(url)
                return
            }
            decisionHandler(.allow)
        }

        func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
            if let url = action.request.url { parent.onLink(url) }
            return nil
        }

        func userContentController(_ c: WKUserContentController, didReceive message: WKScriptMessage) {
            guard let body = message.body as? [String: Any], let type = body["type"] as? String, let id = body["id"] as? String else { return }
            parent.onAction(type, id)
        }
    }
}
