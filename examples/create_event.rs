use core::{ffi::c_void, mem::MaybeUninit, ptr};

use r_efi::efi::{self, Event, EventNotify, Tpl, TPL_APPLICATION};

use mu_rust_helpers::boot_services::{event::EventType, BootServices, StandardBootServices};
use tpl_mutex::TplMutex;

#[derive(Debug)]
struct MyContext {
  some_immutable_state: usize,
  some_other_immutable_state: efi::Handle,
  some_mutable_state: TplMutex<'static, i32>,
  some_other_mutable_state: TplMutex<'static, String>,
}
unsafe impl Sync for MyContext {}

extern "efiapi" fn event_notify_callback_tpl_mutex(_event: Event, context: &'static MyContext) {
  let mut some_mutable_state = context.some_mutable_state.lock();
  *some_mutable_state += 1;
}

extern "efiapi" fn event_notify_callback_tpl_mutex_2(_event: Event, context: Option<&'static MyContext>) {
  println!("{context:?}")
}

static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
const EFI_BOOT_SERVICE: MaybeUninit<efi::BootServices> = MaybeUninit::zeroed();

fn main() {
  BOOT_SERVICE.initialize(unsafe {
    let bs = EFI_BOOT_SERVICE.as_mut_ptr().as_mut().unwrap();
    bs.create_event = efi_create_event;
    bs.raise_tpl = efi_raise_tpl;
    bs.restore_tpl = efi_restore_tpl;
    bs
  });

  let ctx = Box::new(MyContext {
    some_immutable_state: 0,
    some_other_immutable_state: ptr::null_mut(),
    some_mutable_state: TplMutex::new(&BOOT_SERVICE, TPL_APPLICATION, 0),
    some_other_mutable_state: TplMutex::new(&BOOT_SERVICE, TPL_APPLICATION, String::new()),
  });
  let ctx = Box::leak::<'static>(ctx) as &_;

  match BOOT_SERVICE.create_event(
    EventType::RUNTIME | EventType::NOTIFY_SIGNAL,
    TPL_APPLICATION,
    Some(event_notify_callback_tpl_mutex),
    ctx,
  ) {
    Ok(_event) => (),
    Err(_status) => (),
  };

  match BOOT_SERVICE.create_event(
    EventType::RUNTIME | EventType::NOTIFY_SIGNAL,
    TPL_APPLICATION,
    Some(event_notify_callback_tpl_mutex_2),
    Some(ctx),
  ) {
    Ok(_event) => (),
    Err(_status) => (),
  };

  drop(unsafe { Box::from_raw(ctx as *const _ as *mut MyContext) });
}

extern "efiapi" fn efi_create_event(
  _event_type: u32,
  _notify_tpl: Tpl,
  notify_function: Option<EventNotify>,
  notify_context: *mut c_void,
  event: *mut Event,
) -> efi::Status {
  if let Some(notify_function) = notify_function {
    notify_function(ptr::null_mut(), notify_context);
  }
  unsafe { ptr::write(event, ptr::null_mut()) }
  efi::Status::SUCCESS
}

extern "efiapi" fn efi_raise_tpl(_tpl: Tpl) -> Tpl {
  TPL_APPLICATION
}

extern "efiapi" fn efi_restore_tpl(_tpl: Tpl) {}
