//! SDL controller lifetime and button edges. Background events continue while the video owns focus.
use std::{cell::RefCell, ffi::c_void, time::{Duration, Instant}};

/// SDL joystick/controller state is consumed by `poll`. Raw axes/sensor samples
/// (including resting stick noise) are not UI damage; only the synthesized button
/// edges below may request a frame. SDL has already updated its state before here.
pub(crate) fn is_polled_controller_event(event: u32) -> bool {
    matches!(event, 0x600..=0x6ff)
}

#[cfg(test)]
mod tests {
    #[test]
    fn controller_noise_does_not_repaint_but_real_ui_events_do() {
        for event in [0x600, 0x650, 0x651, 0x659] {
            assert!(super::is_polled_controller_event(event));
        }
        for event in [0x100, 0x200, 0x300, 0x301, 0x400, 0x401, 0x402] {
            assert!(!super::is_polled_controller_event(event));
        }
    }
}
unsafe extern "C" {
    fn SDL_NumJoysticks() -> i32;
    fn SDL_IsGameController(index: i32) -> i32;
    fn SDL_GameControllerOpen(index: i32) -> *mut c_void;
    fn SDL_GameControllerClose(controller: *mut c_void);
    fn SDL_GameControllerGetAttached(controller: *mut c_void) -> i32;
    fn SDL_GameControllerGetButton(controller: *mut c_void, button: i32) -> u8;
    fn SDL_GameControllerUpdate();
}
struct Controllers { handles: Vec<*mut c_void>, buttons: [bool; 8], repeat: [Instant; 8], scan: Instant }
impl Drop for Controllers {
    fn drop(&mut self) { for handle in &self.handles { unsafe { SDL_GameControllerClose(*handle); } } }
}
thread_local! { static CONTROLLERS: RefCell<Controllers> = RefCell::new(Controllers { handles: Vec::new(), buttons: [false; 8], repeat: [Instant::now(); 8], scan: Instant::now() - Duration::from_secs(5) }); }
pub(crate) fn poll() -> Vec<String> {
    CONTROLLERS.with(|slot| {
        let mut controllers = slot.borrow_mut(); let now = Instant::now();
        // All SDL calls run on the main thread, and only opened handles reach getters/close.
        unsafe { SDL_GameControllerUpdate(); }
        if now.duration_since(controllers.scan) >= Duration::from_secs(2) {
            controllers.scan = now;
            if controllers.handles.is_empty() || controllers.handles.iter().any(|h| unsafe { SDL_GameControllerGetAttached(*h) } == 0) {
                for handle in controllers.handles.drain(..) { unsafe { SDL_GameControllerClose(handle); } }
                for i in 0..unsafe { SDL_NumJoysticks() } {
                    if unsafe { SDL_IsGameController(i) } != 0 {
                        let handle = unsafe { SDL_GameControllerOpen(i) };
                        if !handle.is_null() { controllers.handles.push(handle); }
                    }
                }
            }
        }
        let mut events = Vec::new();
        for (i, (button, key)) in [(0,"ok"),(1,"back"),(2,"playpause"),(6,"back"),(11,"up"),(12,"down"),(13,"left"),(14,"right")].iter().enumerate() {
            let down = controllers.handles.iter().any(|h| unsafe { SDL_GameControllerGetButton(*h, *button) } != 0);
            if down != controllers.buttons[i] {
                events.push(format!("hold{}:{key}", if down { "down" } else { "up" }));
                controllers.buttons[i] = down; controllers.repeat[i] = now + Duration::from_millis(400);
            } else if down && i >= 4 && now >= controllers.repeat[i] {
                events.push(format!("holdrep:{key}")); controllers.repeat[i] = now + Duration::from_millis(100);
            }
        }
        events
    })
}
