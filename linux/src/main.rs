mod api;
mod app;
mod auth;
mod brand;
mod claude;
mod conversation_html;
mod demo;
mod diff;
mod fix_prompt;
mod host;
mod icons;
mod local_repo;
mod rt;
mod secret;
mod selftest;
mod settings;
mod signal;
mod smoke;
mod theme;
mod ui;
mod updater;
mod util;

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
        return;
    }
    if args.iter().any(|a| a == "--selftest") {
        std::process::exit(selftest::run());
    }
    let smoke = args.iter().any(|a| a == "--smoke");
    let code = app::run(smoke);
    std::process::exit(code.get() as i32);
}
