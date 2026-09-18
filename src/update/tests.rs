//! The machine, driven by events and read through `view`: every rule the
//! menu shows, with no network and no clock but the one handed in.

use std::time::Duration;

use super::*;

const REPO: &str = "https://github.com/Geeooff/GameModeExecutor";

fn release(version: Version) -> Release {
    Release {
        version,
        tag: format!("v{version}"),
        page: format!("{REPO}/releases/tag/v{version}"),
        asset: format!("GameModeExecutor-{version}.msi"),
        sha256: "0".repeat(64),
    }
}

fn labels(items: &[Item]) -> Vec<(&str, bool)> {
    items
        .iter()
        .map(|item| (item.label.as_str(), item.enabled))
        .collect()
}

fn machine() -> (Machine, Instant) {
    (Machine::new(Version(0, 1, 0)), Instant::now())
}

#[test]
fn idle_offers_one_thing_and_a_check_disables_it_while_it_runs() {
    let (mut m, now) = machine();
    assert_eq!(labels(&m.view(now)), vec![("Check for updates", true)]);
    assert_eq!(m.view(now)[0].action, Some(Action::Check));

    assert_eq!(m.apply(Event::CheckAsked, now), Some(Effect::Check));
    assert_eq!(
        labels(&m.view(now)),
        vec![("Checking for updates\u{2026}", false)]
    );
    assert_eq!(m.apply(Event::CheckAsked, now), None, "one check at a time");
}

#[test]
fn up_to_date_is_said_for_an_hour_and_then_not() {
    let (mut m, now) = machine();
    m.apply(Event::CheckAsked, now);
    assert_eq!(m.apply(Event::CheckDone(Ok(Verdict::UpToDate)), now), None);
    assert_eq!(
        labels(&m.view(now)),
        vec![
            ("Check for updates", true),
            ("0.1.0 is the latest version", false)
        ]
    );
    let later = now + VERDICT_TTL - Duration::from_secs(1);
    assert_eq!(m.view(later).len(), 2, "still within the hour");
    let expired = now + VERDICT_TTL;
    assert_eq!(labels(&m.view(expired)), vec![("Check for updates", true)]);
}

#[test]
fn a_release_found_is_offered_until_acted_on() {
    let (mut m, now) = machine();
    m.apply(Event::CheckAsked, now);
    let found = release(Version(0, 2, 0));
    m.apply(Event::CheckDone(Ok(Verdict::Available(found.clone()))), now);
    let items = m.view(now);
    assert_eq!(
        labels(&items),
        vec![
            ("Check for updates", true),
            ("Download and install 0.2.0", true),
            ("What changed in 0.2.0", true),
        ]
    );
    assert_eq!(items[1].action, Some(Action::Install));
    assert_eq!(
        items[2].action,
        Some(Action::OpenReleasePage(found.page.clone()))
    );
    let days_later = now + Duration::from_secs(2 * 24 * 3600);
    assert_eq!(m.view(days_later).len(), 3, "a release does not un-release");
}

#[test]
fn installing_downloads_then_installs_and_says_where_it_is() {
    let (mut m, now) = machine();
    m.apply(Event::CheckAsked, now);
    let found = release(Version(0, 2, 0));
    m.apply(Event::CheckDone(Ok(Verdict::Available(found.clone()))), now);

    assert_eq!(
        m.apply(Event::InstallAsked, now),
        Some(Effect::Download(found.clone()))
    );
    assert_eq!(
        labels(&m.view(now)),
        vec![
            ("Check for updates", false),
            ("Downloading 0.2.0\u{2026}", false),
            ("What changed in 0.2.0", true),
        ]
    );
    m.apply(
        Event::DownloadStarted {
            size: Some(1_462_272),
        },
        now,
    );
    assert_eq!(m.view(now)[1].label, "Downloading 0.2.0 (1.5 MB)\u{2026}");
    assert_eq!(
        m.apply(Event::CheckAsked, now),
        None,
        "no check mid-download"
    );

    assert_eq!(
        m.apply(Event::DownloadDone(Ok(())), now),
        Some(Effect::Install(found))
    );
    assert_eq!(
        labels(&m.view(now)),
        vec![
            ("Check for updates", false),
            ("Installing 0.2.0\u{2026}", false)
        ]
    );
}

#[test]
fn a_failed_download_is_said_and_expires() {
    let (mut m, now) = machine();
    m.apply(Event::CheckAsked, now);
    m.apply(
        Event::CheckDone(Ok(Verdict::Available(release(Version(0, 2, 0))))),
        now,
    );
    m.apply(Event::InstallAsked, now);
    assert_eq!(
        m.apply(Event::DownloadDone(Err(Fault::Verification)), now),
        None
    );
    assert_eq!(
        labels(&m.view(now)),
        vec![
            ("Check for updates", true),
            ("Download failed: the file did not verify (see log)", false),
        ]
    );
    assert_eq!(
        labels(&m.view(now + VERDICT_TTL)),
        vec![("Check for updates", true)]
    );
}

#[test]
fn a_check_failure_names_its_cause() {
    for (fault, line) in [
        (
            Fault::NoConnection { code: 12007 },
            "Could not check: no connection (see log)",
        ),
        (
            Fault::Http { status: 503 },
            "Could not check: GitHub answered 503 (see log)",
        ),
        (
            Fault::Unexpected("a portal".into()),
            "Could not check: unexpected answer (see log)",
        ),
    ] {
        let (mut m, now) = machine();
        m.apply(Event::CheckAsked, now);
        m.apply(Event::CheckDone(Err(fault)), now);
        assert_eq!(m.view(now)[1].label, line);
    }
}

#[test]
fn an_installer_refusal_is_said_with_its_code() {
    let (mut m, now) = machine();
    m.apply(Event::CheckAsked, now);
    m.apply(
        Event::CheckDone(Ok(Verdict::Available(release(Version(0, 2, 0))))),
        now,
    );
    m.apply(Event::InstallAsked, now);
    m.apply(Event::DownloadDone(Ok(())), now);
    m.apply(Event::InstallFailed(Fault::Installer { code: 1618 }), now);
    assert_eq!(
        m.view(now)[1].label,
        "Update failed: Windows Installer 1618 (see log)"
    );
    assert_eq!(
        m.apply(Event::CheckAsked, now),
        Some(Effect::Check),
        "and a new check clears it"
    );
}

#[test]
fn a_failure_found_at_start_stays_until_the_next_check() {
    let (mut m, now) = machine();
    m.apply(
        Event::FoundAtStart(Fault::Setup(
            "Update to 0.2.0 failed: Windows Installer 1603".into(),
        )),
        now,
    );
    let line = "Update to 0.2.0 failed: Windows Installer 1603 (see log)";
    assert_eq!(m.view(now)[1].label, line);
    let days_later = now + Duration::from_secs(2 * 24 * 3600);
    assert_eq!(m.view(days_later)[1].label, line, "no expiry");
    assert_eq!(m.apply(Event::CheckAsked, days_later), Some(Effect::Check));
    assert_eq!(m.view(days_later).len(), 1);
}

#[test]
fn a_new_check_clears_an_offer_and_stale_answers_are_ignored() {
    let (mut m, now) = machine();
    m.apply(Event::CheckAsked, now);
    m.apply(
        Event::CheckDone(Ok(Verdict::Available(release(Version(0, 2, 0))))),
        now,
    );
    assert_eq!(m.apply(Event::CheckAsked, now), Some(Effect::Check));
    assert_eq!(
        labels(&m.view(now)),
        vec![("Checking for updates\u{2026}", false)]
    );

    let (mut m, now) = machine();
    assert_eq!(m.apply(Event::CheckDone(Ok(Verdict::UpToDate)), now), None);
    assert_eq!(
        m.phase(),
        &Phase::Idle,
        "an answer nobody asked for changes nothing"
    );
    assert_eq!(m.apply(Event::InstallAsked, now), None);
    assert_eq!(m.apply(Event::DownloadDone(Ok(())), now), None);
    assert_eq!(m.phase(), &Phase::Idle);
}

#[test]
fn every_fault_has_a_menu_line_and_a_log_sentence() {
    let faults = [
        Fault::NoConnection { code: 12002 },
        Fault::Http { status: 429 },
        Fault::Unexpected("html".into()),
        Fault::Verification,
        Fault::Write {
            path: std::path::PathBuf::from(r"C:\u\updates"),
            detail: "disk full".into(),
        },
        Fault::Installer { code: 1603 },
        Fault::Setup("Update to 0.2.0 failed: zip: no such folder".into()),
    ];
    for fault in &faults {
        let line = fault.menu_line("download");
        assert!(line.ends_with("(see log)"), "{line}");
        assert!(!fault.to_string().is_empty());
    }
    assert_eq!(
        faults[4].menu_line("download"),
        r"Download failed: cannot write to C:\u\updates (see log)"
    );
    assert_eq!(faults[0].to_string(), "no connection (WinHTTP error 12002)");
    assert_eq!(megabytes(1_462_272), "1.5 MB");
}

/// The two tests that go through the process-wide state take turns: the
/// state is one per process, and the test harness runs tests in parallel.
static PROCESS_WIDE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn the_process_wide_updater_renders_its_section_once_set_up() {
    // Before `start`, nothing: the tests of everything else see no
    // section. After it, the idle entry, and an action the phase does not
    // take is ignored without a thread being spawned.
    let _turn = PROCESS_WIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let context = scratch_context();
    start(context);
    assert_eq!(labels(&view()), vec![("Check for updates", true)]);
    perform(Action::Install);
    assert_eq!(labels(&view()), vec![("Check for updates", true)]);
    perform(Action::OpenReleasePage("https://example.invalid".into()));
    assert_eq!(labels(&view()), vec![("Check for updates", true)]);
}

#[test]
fn a_failure_left_behind_is_shown_when_the_updater_is_set_up() {
    let context = scratch_context();
    std::fs::create_dir_all(&context.updates_dir).unwrap();
    std::fs::write(context.updates_dir.join("pending.txt"), "99.0.0").unwrap();
    std::fs::write(
        context.updates_dir.join("result.txt"),
        "Windows Installer 1603",
    )
    .unwrap();
    let mut machine = Machine::new(Version::running());
    if let install::Settled::Failed(fault) = install::settle(&context) {
        machine.apply(Event::FoundAtStart(fault), Instant::now());
    }
    assert_eq!(
        machine.view(Instant::now())[1].label,
        "Update to 99.0.0 failed: Windows Installer 1603 (see log)"
    );
    assert_eq!(
        machine.take_notice().unwrap().title,
        "The last update failed"
    );
}

#[test]
fn a_version_running_for_the_first_time_after_an_update_says_so() {
    // The install itself goes by in a second, so this is the moment the
    // new version is seen: a notice, the menu unchanged, and the tray woken
    // for it once the updater starts.
    let _turn = PROCESS_WIDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut m, now) = machine();
    assert_eq!(m.apply(Event::UpdatedAtStart(Version(0, 2, 0)), now), None);
    let notice = m.take_notice().expect("told once");
    assert_eq!(notice.title, "Updated to 0.2.0");
    assert_eq!(notice.text, "GameModeExecutor is running the new version.");
    assert_eq!(labels(&m.view(now)), vec![("Check for updates", true)]);

    let woken = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut context = scratch_context();
    let flag = Arc::clone(&woken);
    context.wake = Some(Arc::new(move || {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    std::fs::create_dir_all(&context.updates_dir).unwrap();
    std::fs::write(
        context.updates_dir.join("pending.txt"),
        Version::running().to_string(),
    )
    .unwrap();
    start(context);
    assert!(
        woken.load(std::sync::atomic::Ordering::SeqCst),
        "the tray was woken"
    );
    assert_eq!(
        take_notice().unwrap().title,
        format!("Updated to {}", Version::running())
    );
    assert_eq!(take_notice(), None);
}

#[test]
fn a_copy_is_installed_only_when_the_package_owns_its_folder() {
    let package = std::path::Path::new(r"C:\Users\me\AppData\Local\Programs\GameModeExecutor");
    assert_eq!(kind_of(package, true, Some(package)), Kind::Installer);
    assert_eq!(
        kind_of(
            std::path::Path::new(r"c:\users\me\appdata\local\programs\gamemodeexecutor\"),
            true,
            Some(package)
        ),
        Kind::Installer,
        "case and a trailing separator do not matter"
    );
    assert_eq!(
        kind_of(std::path::Path::new(r"C:\Tools\GME"), true, Some(package)),
        Kind::Zip,
        "an unpacked copy beside a package updates itself, not the package"
    );
    assert_eq!(kind_of(package, false, Some(package)), Kind::Zip);
    assert_eq!(kind_of(package, true, None), Kind::Zip);
}

#[test]
fn the_context_of_this_process_names_a_repository_and_a_folder() {
    let context = Context::of_this_process(None, None).unwrap();
    assert!(context.repository.starts_with("https://github.com/"));
    assert!(context.updates_dir.ends_with("updates"));
    assert!(context.install_dir.is_dir());
    // Decided when asked, not at start: the test binary is no package.
    assert_eq!(context.kind, None);
    assert_eq!(context.kind(), Kind::Zip);
}

/// A feed that serves bytes from memory, for the download path.
struct Bytes(Vec<u8>);

impl feed::Feed for Bytes {
    fn redirect_of(&self, _url: &str) -> Result<String, Fault> {
        unreachable!()
    }
    fn text(&self, _url: &str) -> Result<String, Fault> {
        unreachable!()
    }
    fn download(
        &self,
        _url: &str,
        to: &std::path::Path,
        progress: &mut dyn FnMut(Option<u64>),
    ) -> Result<u64, Fault> {
        progress(Some(self.0.len() as u64));
        std::fs::write(to, &self.0).unwrap();
        Ok(self.0.len() as u64)
    }
}

fn scratch_context() -> Context {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gme-download-{}-{n}", std::process::id()));
    Context {
        repository: REPO.to_owned(),
        kind: Some(Kind::Installer),
        updates_dir: dir.join("updates"),
        install_dir: dir.join("program"),
        stop: None,
        wake: None,
    }
}

#[test]
fn every_outcome_leaves_one_notice_and_a_click_leaves_none() {
    let (mut m, now) = machine();
    assert_eq!(m.take_notice(), None);
    m.apply(Event::CheckAsked, now);
    assert_eq!(m.take_notice(), None, "asking is not an outcome");

    m.apply(Event::CheckDone(Ok(Verdict::UpToDate)), now);
    let notice = m.take_notice().expect("a verdict is told");
    assert_eq!(notice.title, "Up to date");
    assert_eq!(notice.text, "0.1.0 is the latest version.");
    assert_eq!(m.take_notice(), None, "told once");

    m.apply(Event::CheckAsked, now);
    m.apply(
        Event::CheckDone(Ok(Verdict::Available(release(Version(0, 2, 0))))),
        now,
    );
    let notice = m.take_notice().unwrap();
    assert_eq!(notice.title, "Update available");
    assert!(notice.text.starts_with("0.2.0 is available."));

    m.apply(Event::InstallAsked, now);
    assert_eq!(m.take_notice(), None);
    m.apply(Event::DownloadDone(Ok(())), now);
    assert_eq!(m.take_notice().unwrap().title, "Installing 0.2.0");
    m.apply(Event::InstallFailed(Fault::Installer { code: 1618 }), now);
    let notice = m.take_notice().unwrap();
    assert_eq!(notice.title, "Update failed");
    assert_eq!(
        notice.text,
        "Windows Installer exited with 1618. See the log."
    );

    let (mut m, now) = machine();
    m.apply(Event::CheckAsked, now);
    m.apply(
        Event::CheckDone(Err(Fault::NoConnection { code: 12007 })),
        now,
    );
    let notice = m.take_notice().unwrap();
    assert_eq!(notice.title, "Could not check for updates");
    assert_eq!(
        notice.text,
        "no connection (WinHTTP error 12007). See the log."
    );
}

#[test]
fn a_download_is_kept_only_when_it_hashes_to_what_the_release_says() {
    let context = scratch_context();
    let mut found = release(Version(0, 2, 0));
    // "abc", whose SHA-256 the standard publishes.
    found.sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned();
    assert_eq!(download(&Bytes(b"abc".to_vec()), &context, &found), Ok(()));
    assert!(context.updates_dir.join(&found.asset).exists());

    assert_eq!(
        download(&Bytes(b"abd".to_vec()), &context, &found),
        Err(Fault::Verification)
    );
    assert!(
        !context.updates_dir.join(&found.asset).exists(),
        "a file that does not verify is deleted before anything can run it"
    );
}
