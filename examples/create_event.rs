use core::{ffi::c_void, mem::MaybeUninit, ptr};

use boot_services::{event::NoContext, tpl::Tpl};
use r_efi::efi::{self, Event};

use mu_rust_helpers::boot_services::{event::EventType, BootServices, StandardBootServices};
use tpl_mutex::TplMutex;

#[derive(Debug)]
struct MyContext {
  _some_immutable_state: usize,
  _some_other_immutable_state: efi::Handle,
  some_mutable_state: TplMutex<'static, i32>,
  _some_other_mutable_state: TplMutex<'static, String>,
}
unsafe impl Sync for MyContext {}

extern "efiapi" fn event_notify_callback_tpl_mutex(_event: Event, context: &'static MyContext) {
  let mut some_mutable_state = context.some_mutable_state.lock();
  *some_mutable_state += 1;
}

extern "efiapi" fn event_notify_callback_tpl_mutex_2(_event: Event, context: Option<&'static MyContext>) {
  println!("{context:?}")
}

extern "efiapi" fn event_notify_callback_void(_event: Event, context: NoContext) {
  println!("{context:?}")
}

static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
const EFI_BOOT_SERVICE: *mut efi::BootServices = ptr::null_mut();

fn main() {
  unsafe {
    let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
    bs.assume_init_mut().create_event = efi_create_event;
    bs.assume_init_mut().raise_tpl = efi_raise_tpl;
    bs.assume_init_mut().restore_tpl = efi_restore_tpl;
    ptr::write(EFI_BOOT_SERVICE, bs.assume_init());
  }
  BOOT_SERVICE.initialize(unsafe { EFI_BOOT_SERVICE.as_ref::<'static>().unwrap() });
// 
  let ctx = Box::new(MyContext {
    _some_immutable_state: 0,
    _some_other_immutable_state: ptr::null_mut(),
    some_mutable_state: TplMutex::new(&BOOT_SERVICE, Tpl::APPLICATION, 0),
    _some_other_mutable_state: TplMutex::new(&BOOT_SERVICE, Tpl::APPLICATION, String::new()),
  });

  let ctx = Box::leak::<'static>(ctx) as &_;

  match BOOT_SERVICE.create_event(
    EventType::RUNTIME | EventType::NOTIFY_SIGNAL,
    Tpl::APPLICATION,
    Some(event_notify_callback_tpl_mutex),
    ctx,
  ) {
    Ok(_event) => (),
    Err(_status) => (),
  };

  match BOOT_SERVICE.create_event(
    EventType::RUNTIME | EventType::NOTIFY_SIGNAL,
    Tpl::APPLICATION,
    Some(event_notify_callback_tpl_mutex_2),
    Some(ctx),
  ) {
    Ok(_event) => (),
    Err(_status) => (),
  };

  match BOOT_SERVICE.create_event(EventType::RUNTIME, Tpl::APPLICATION, Some(event_notify_callback_void), ()) {
    Ok(_event) => (),
    Err(_status) => (),
  };

  drop(unsafe { Box::from_raw(ctx as *const _ as *mut MyContext) });
}

extern "efiapi" fn efi_create_event(
  _event_type: u32,
  _notify_tpl: efi::Tpl,
  notify_function: Option<efi::EventNotify>,
  notify_context: *mut c_void,
  event: *mut Event,
) -> efi::Status {
  if let Some(notify_function) = notify_function {
    notify_function(ptr::null_mut(), notify_context);
  }
  unsafe { ptr::write(event, ptr::null_mut()) }
  efi::Status::SUCCESS
}

extern "efiapi" fn efi_raise_tpl(_tpl: efi::Tpl) -> efi::Tpl {
  efi::TPL_APPLICATION
}

extern "efiapi" fn efi_restore_tpl(_tpl: efi::Tpl) {}
