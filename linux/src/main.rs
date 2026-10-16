mod api;
mod app;
mod auth;
mod brand;
mod claude;
mod conversation_html;
mod demo;
mod diagnostics;
mod diff;
mod fix_prompt;
mod fixtures;
mod host;
mod icons;
mod local_repo;
mod rt;
mod secret;
mod selftest;
mod settings;
mod signal;
mod smoke;
mod tour;
mod theme;
mod ui;
mod updater;
mod util;

fn webkit_sandbox_guard() {
    if std::env::var_os("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS").is_some() || util::is_flatpak() {
        return;
    }
    let opt_out = brand::env_key("DISABLE_WEBKIT_SANDBOX");
    if std::env::var(&opt_out).map(|v| v == "1").unwrap_or(false) {
        std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
        return;
    }
    let restricted = std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns").map(|s| s.trim() == "1").unwrap_or(false);
    if !restricted {
        return;
    }
    let works = std::process::Command::new("bwrap")
        .args(["--unshare-user", "--ro-bind", "/", "/", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !works {
        eprintln!("The WebKit sandbox cannot start because AppArmor restricts unprivileged user namespaces. If rendered content stays blank, allow bwrap in AppArmor or set {opt_out}=1 to run the web view without its sandbox (unsafe).");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", brand::APP_NAME, brand::VERSION);
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{} {} — {}", brand::APP_NAME, brand::VERSION, brand::TAGLINE);
        println!();
        println!("Usage: {} [OPTIONS] [URL…]", brand::BIN_NAME);
        println!();
        println!("  URL            a github.com link or {}:// deep link to open", brand::URL_SCHEME);
        println!("  --version      print the version and exit");
        println!("  --selftest     run a read-only API self test ({} required)", brand::env_key("TOKEN"));
        println!("  --smoke        build every screen offline and exit (used by CI)");
        println!("  --tour=DIR     walk the main screens and save screenshots to DIR (used by CI)");
        return;
    }
    if args.iter().any(|a| a == "--selftest") {
        std::process::exit(selftest::run());
    }
    webkit_sandbox_guard();
    if let Some(dir) = args.iter().find_map(|a| a.strip_prefix("--tour=")) {
        tour::set_dir(std::path::PathBuf::from(dir));
    }
    let smoke = args.iter().any(|a| a == "--smoke");
    let code = app::run(smoke);
    std::process::exit(code.get() as i32);
}
