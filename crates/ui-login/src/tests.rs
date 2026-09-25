use super::*;
use std::vec::Vec;

/// A stand-in for tepOS's AuthenticationService: delays from the fifth
/// failure, locked at the tenth, set only when nothing is set.
struct FakeEnclave {
    reachable: bool,
    passcode: Option<Vec<u8>>,
    failures: u32,
    wait: u32,
    verify_calls: u32,
    set_calls: Vec<Vec<u8>>,
    /// Force this answer from verify/set instead of the rules above.
    answer: Option<AuthResult>,
}

impl FakeEnclave {
    fn new(passcode: Option<&[u8]>) -> Self {
        Self {
            reachable: true,
            passcode: passcode.map(|bytes| bytes.to_vec()),
            failures: 0,
            wait: 0,
            verify_calls: 0,
            set_calls: Vec::new(),
            answer: None,
        }
    }
}

impl PasscodeChecker for FakeEnclave {
    fn status(&mut self) -> Option<AuthStatus> {
        self.reachable.then(|| AuthStatus {
            passcode_set: u32::from(self.passcode.is_some()),
            failures: self.failures,
            locked: u32::from(self.failures >= 10),
            wait_seconds: self.wait,
        })
    }

    fn verify(&mut self, passcode: &[u8]) -> (AuthResult, u32) {
        self.verify_calls += 1;
        if let Some(answer) = self.answer {
            return (answer, 7);
        }
        if !self.reachable {
            return (AuthResult::Unavailable, 0);
        }
        if self.failures >= 10 {
            return (AuthResult::Locked, 0);
        }
        if self.wait != 0 {
            return (AuthResult::RetryLater, self.wait);
        }
        match &self.passcode {
            None => (AuthResult::NotSet, 0),
            Some(stored) if stored.as_slice() == passcode => {
                self.failures = 0;
                (AuthResult::Ok, 0)
            }
            Some(_) => {
                self.failures += 1;
                if self.failures >= 5 {
                    self.wait = 60;
                }
                (if self.failures >= 10 { AuthResult::Locked } else { AuthResult::Denied }, 0)
            }
        }
    }

    fn set(&mut self, passcode: &[u8]) -> (AuthResult, u32) {
        self.set_calls.push(passcode.to_vec());
        if let Some(answer) = self.answer {
            return (answer, 0);
        }
        if !self.reachable {
            return (AuthResult::Unavailable, 0);
        }
        if self.passcode.is_some() {
            return (AuthResult::Denied, 0);
        }
        self.passcode = Some(passcode.to_vec());
        (AuthResult::Ok, 0)
    }
}

fn press(screen: &mut LoginScreen, code: u32, character: Option<char>, now: u64) -> bool {
    screen.event(Event::KeyDown { code, character }, now)
}

fn type_text(screen: &mut LoginScreen, text: &str, now: u64) {
    for character in text.chars() {
        press(screen, 0x1000, Some(character), now);
    }
}

fn enter(screen: &mut LoginScreen, enclave: &mut FakeEnclave, now: u64) {
    press(screen, key::ENTER, None, now);
    if screen.has_pending() {
        screen.run_pending(enclave, now);
    }
}

/// A screen that has come through the greeting into Setup.
fn at_setup(enclave: &mut FakeEnclave) -> LoginScreen {
    let mut screen = LoginScreen::new();
    screen.start(enclave, 0);
    assert_eq!(screen.phase(), Phase::Greeting);
    screen.tick(enclave, GREETING_US + 1);
    assert_eq!(screen.phase(), Phase::Setup);
    screen
}

#[test]
fn first_boot_greets_then_sets_the_typed_passcode() {
    let mut enclave = FakeEnclave::new(None);
    let mut screen = at_setup(&mut enclave);
    let now = GREETING_US + 2;

    type_text(&mut screen, "correct horse", now);
    press(&mut screen, key::TAB, None, now);
    type_text(&mut screen, "correct horse", now);
    enter(&mut screen, &mut enclave, now);

    assert_eq!(enclave.set_calls, [b"correct horse".to_vec()]);
    assert!(screen.finished());
    assert!(screen.passcode.is_empty() && screen.confirm.is_empty(), "passcode left in memory");
}

#[test]
fn the_greeting_ignores_keys_and_runs_its_course() {
    let mut enclave = FakeEnclave::new(None);
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 0);
    assert!(!press(&mut screen, key::ENTER, None, 10));
    screen.tick(&mut enclave, GREETING_US / 2);
    assert_eq!(screen.phase(), Phase::Greeting);
    assert!(enclave.set_calls.is_empty());
}

#[test]
fn setup_refuses_short_and_mismatched_passcodes_without_asking_the_enclave() {
    let mut enclave = FakeEnclave::new(None);
    let mut screen = at_setup(&mut enclave);

    type_text(&mut screen, "abc", 1);
    press(&mut screen, key::TAB, None, 1);
    type_text(&mut screen, "abc", 1);
    enter(&mut screen, &mut enclave, 1);
    assert_eq!(screen.note, Note::Error("Use at least 4 characters."));

    let mut screen = at_setup(&mut enclave);
    type_text(&mut screen, "abcd1234", 1);
    press(&mut screen, key::TAB, None, 1);
    type_text(&mut screen, "abcd1235", 1);
    enter(&mut screen, &mut enclave, 1);
    assert_eq!(screen.note, Note::Error("The passcodes do not match."));
    assert!(screen.confirm.is_empty());

    assert!(enclave.set_calls.is_empty());
    assert!(!screen.finished());
}

#[test]
fn enter_on_the_first_field_moves_to_confirm() {
    let mut enclave = FakeEnclave::new(None);
    let mut screen = at_setup(&mut enclave);
    type_text(&mut screen, "abcd1234", 1);
    enter(&mut screen, &mut enclave, 1);
    assert_eq!(screen.field, 1);
    assert!(enclave.set_calls.is_empty());
}

#[test]
fn the_right_passcode_unlocks() {
    let mut enclave = FakeEnclave::new(Some(b"hunter22"));
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 0);
    assert_eq!(screen.phase(), Phase::Login);

    type_text(&mut screen, "hunter22", 1);
    enter(&mut screen, &mut enclave, 1);
    assert!(screen.finished());
}

#[test]
fn a_wrong_passcode_shakes_clears_and_stays_locked() {
    let mut enclave = FakeEnclave::new(Some(b"hunter22"));
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 0);

    type_text(&mut screen, "hunter23", 1);
    enter(&mut screen, &mut enclave, 1);
    assert!(!screen.finished());
    assert_eq!(screen.note, Note::Error("Wrong passcode."));
    assert!(screen.passcode.is_empty());
    assert!(screen.shake_started_us.is_some());
}

#[test]
fn the_fifth_failure_starts_a_countdown_that_blocks_typing() {
    let mut enclave = FakeEnclave::new(Some(b"hunter22"));
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 0);

    for _ in 0..5 {
        type_text(&mut screen, "wrong-one", 1);
        enter(&mut screen, &mut enclave, 1);
    }
    assert_eq!(screen.note, Note::Wait);
    assert_eq!(screen.wait_until_us, 1 + 60_000_000);

    // Typing and Enter do nothing during the wait; the enclave is not asked.
    let calls = enclave.verify_calls;
    type_text(&mut screen, "hunter22", 2);
    enter(&mut screen, &mut enclave, 2);
    assert_eq!(enclave.verify_calls, calls);
    assert!(screen.passcode.is_empty());

    // When it runs out, the screen asks again and lets the next try through.
    enclave.wait = 0;
    screen.tick(&mut enclave, 60_000_002);
    assert_eq!(screen.note, Note::None);
    type_text(&mut screen, "hunter22", 60_000_003);
    enter(&mut screen, &mut enclave, 60_000_003);
    assert!(screen.finished());
}

#[test]
fn a_delay_already_running_at_boot_is_shown() {
    let mut enclave = FakeEnclave::new(Some(b"hunter22"));
    enclave.wait = 30;
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 5);
    assert_eq!(screen.phase(), Phase::Login);
    assert_eq!(screen.note, Note::Wait);
    let mut buffer = [0u8; 48];
    assert_eq!(format_wait(30, &mut buffer), "Too many attempts. Try again in 0:30.");
    assert_eq!(format_wait(3600, &mut buffer), "Too many attempts. Try again in 60:00.");
}

#[test]
fn a_locked_passcode_offers_no_field_and_waits_for_a_reset() {
    let mut enclave = FakeEnclave::new(Some(b"hunter22"));
    enclave.failures = 10;
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 0);
    assert_eq!(screen.phase(), Phase::Locked);
    assert!(!press(&mut screen, 0x1000, Some('a'), 1));

    // A recovery reset on the enclave's side clears everything.
    enclave.failures = 0;
    enclave.passcode = None;
    screen.tick(&mut enclave, LOCKED_POLL_US + 1);
    assert_eq!(screen.phase(), Phase::Greeting);
}

#[test]
fn an_unreachable_enclave_never_lets_anyone_in() {
    let mut enclave = FakeEnclave::new(Some(b"hunter22"));
    enclave.reachable = false;
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 0);
    assert_eq!(screen.phase(), Phase::Waiting);
    assert!(!press(&mut screen, key::ENTER, None, 1));

    screen.tick(&mut enclave, WAITING_POLL_US + 1);
    assert_eq!(screen.phase(), Phase::Waiting);

    enclave.reachable = true;
    screen.tick(&mut enclave, 2 * WAITING_POLL_US + 2);
    assert_eq!(screen.phase(), Phase::Login);
}

#[test]
fn the_enclave_going_away_mid_login_goes_back_to_waiting() {
    let mut enclave = FakeEnclave::new(Some(b"hunter22"));
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 0);
    enclave.reachable = false;
    type_text(&mut screen, "hunter22", 1);
    enter(&mut screen, &mut enclave, 1);
    assert_eq!(screen.phase(), Phase::Waiting);
    assert!(!screen.finished());
}

#[test]
fn only_an_explicit_ok_finishes() {
    for answer in [
        AuthResult::Denied,
        AuthResult::RetryLater,
        AuthResult::Locked,
        AuthResult::Unavailable,
        AuthResult::NotSet,
        AuthResult::Invalid,
        AuthResult::Error,
        AuthResult::from_raw(0x5EC),
    ] {
        let mut enclave = FakeEnclave::new(Some(b"hunter22"));
        let mut screen = LoginScreen::new();
        screen.start(&mut enclave, 0);
        enclave.answer = Some(answer);
        type_text(&mut screen, "hunter22", 1);
        enter(&mut screen, &mut enclave, 1);
        assert!(!screen.finished(), "{answer:?} let the user in");
    }
}

#[test]
fn a_passcode_set_elsewhere_during_setup_asks_for_that_one() {
    let mut enclave = FakeEnclave::new(None);
    let mut screen = at_setup(&mut enclave);
    enclave.passcode = Some(b"someone-else".to_vec());
    type_text(&mut screen, "mine-1234", 1);
    press(&mut screen, key::TAB, None, 1);
    type_text(&mut screen, "mine-1234", 1);
    enter(&mut screen, &mut enclave, 1);
    assert_eq!(screen.phase(), Phase::Login);
    assert!(!screen.finished());
}

#[test]
fn passcodes_stop_at_64_characters_and_only_take_printable_ascii() {
    let mut enclave = FakeEnclave::new(Some(b"hunter22"));
    let mut screen = LoginScreen::new();
    screen.start(&mut enclave, 0);
    for _ in 0..70 {
        press(&mut screen, 0x1000, Some('x'), 1);
    }
    assert_eq!(screen.passcode.len(), 64);
    screen.passcode.wipe();
    press(&mut screen, 0x1000, Some('é'), 1);
    press(&mut screen, 0x1000, Some('\n'), 1);
    assert!(screen.passcode.is_empty());
    press(&mut screen, 0x1000, Some('a'), 1);
    press(&mut screen, key::BACKSPACE, None, 1);
    assert!(screen.passcode.is_empty());
}

#[test]
fn a_wiped_secret_is_zero() {
    let mut secret = Secret::new();
    for byte in b"hunter22" {
        secret.push(*byte);
    }
    secret.wipe();
    assert!(secret.bytes.iter().all(|&byte| byte == 0));
}

#[test]
fn the_backdrop_blurs_and_darkens() {
    // Half black, half white: the blurred edge must have in-between values,
    // and white must come out darker than white.
    let (width, height) = (64u32, 32u32);
    let mut source: Vec<u32> = (0..width * height)
        .map(|index| if index % width < width / 2 { 0 } else { 0x00FF_FFFF })
        .collect();
    let surface = Surface::new(&mut source, width, height, width).unwrap();
    let low = Backdrop::low_size(Size::new(width, height));
    let mut buffer = std::vec![0u32; (low.width * low.height) as usize];
    let mut temp = buffer.clone();
    let backdrop = Backdrop::build(&surface, &mut buffer, &mut temp).unwrap();

    let mut out = std::vec![0u32; (width * height) as usize];
    let mut target = Surface::new(&mut out, width, height, width).unwrap();
    backdrop.paint(&mut target, Rect::new(0, 0, width, height));

    let row: Vec<u32> = out[(16 * width) as usize..(17 * width) as usize].iter().map(|pixel| pixel & 0xFF).collect();
    assert!(row[width as usize - 1] < 0xFF && row[width as usize - 1] > 0x90, "{row:?}");
    assert!(row.iter().any(|&value| value > 20 && value < 150), "no soft edge: {row:?}");
    assert!(row.windows(2).all(|pair| pair[0] <= pair[1]), "not monotonic: {row:?}");
}
