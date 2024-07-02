#![cfg_attr(all(not(test), not(feature = "mockall")), no_std)]
extern crate alloc;

pub mod event;
pub mod tpl;

#[cfg(any(test, feature = "mockall"))]
use mockall::automock;

use core::{
  ffi::c_void,
  marker::PhantomData,
  mem::{self, MaybeUninit},
  option::Option,
  ptr,
  sync::atomic::{AtomicPtr, Ordering},
};

use r_efi::efi;

use event::{EventCtxMutPtr, EventNotifyCallback, EventTimerType, EventType};
use tpl::Tpl;

#[cfg_attr(any(test, feature = "mockall"), automock)]
pub trait BootServices {
  fn create_event<T: EventCtxMutPtr<Ctx, FFIType = FFIType> + 'static, Ctx: Sized + 'static, FFIType: 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventNotifyCallback<FFIType>>,
    notify_context: T,
  ) -> Result<efi::Event, efi::Status> {
    unsafe {
      self.create_event_unchecked(
        event_type,
        notify_tpl,
        mem::transmute(notify_function),
        notify_context.into_raw_mut(),
      )
    }
  }

  fn create_event_ex<T: EventCtxMutPtr<Ctx, FFIType = FFIType> + 'static, Ctx: Sized + 'static, FFIType: 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventNotifyCallback<FFIType>>,
    notify_context: T,
    event_group: Option<&'static efi::Guid>,
  ) -> Result<efi::Event, efi::Status> {
    unsafe {
      self.create_event_ex_unchecked(
        event_type,
        notify_tpl.into(),
        mem::transmute(notify_function),
        notify_context.into_raw_mut(),
        event_group,
      )
    }
  }

  unsafe fn create_event_unchecked<T: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventNotifyCallback<*mut T>>,
    notify_context: *mut T,
  ) -> Result<efi::Event, efi::Status>;

  unsafe fn create_event_ex_unchecked<T: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: EventNotifyCallback<*mut T>,
    notify_context: *mut T,
    event_group: Option<&'static efi::Guid>,
  ) -> Result<efi::Event, efi::Status>;

  fn close_event(&self, event: efi::Event) -> Result<(), efi::Status>;

  fn signal_event(&self, event: efi::Event) -> Result<(), efi::Status>;

  fn wait_for_event(&self, events: &mut [efi::Event]) -> Result<usize, efi::Status>;

  fn check_event(&self, event: efi::Event) -> Result<(), efi::Status>;

  fn set_timer(&self, event: efi::Event, timer_type: EventTimerType, trigger_time: u64) -> Result<(), efi::Status>;

  /// Raises a task’s priority level and returns its previous level.
  fn raise_tpl(&self, tpl: Tpl) -> Tpl;
  /// Restores a task’s priority level to its previous value.
  fn restore_tpl(&self, tpl: Tpl);
}

#[derive(Debug)]
pub struct StandardBootServices<'a> {
  efi_boot_services: AtomicPtr<efi::BootServices>,
  _lifetime_marker: PhantomData<&'a efi::BootServices>,
}

impl<'a> StandardBootServices<'a> {
  /// Create a new StandardBootServices with the provided [efi::BootServices].
  pub const fn new(efi_boot_services: &'a efi::BootServices) -> Self {
    // The efi::BootServices is only read, that is why we use a non mutable reference.
    Self { efi_boot_services: AtomicPtr::new(efi_boot_services as *const _ as *mut _), _lifetime_marker: PhantomData }
  }

  /// Create a new StandardBootServices that is uninitialized.
  /// The struct need to be initialize later with [Self::initialize], otherwise, subsequent call will panic.
  pub const fn new_uninit() -> Self {
    Self { efi_boot_services: AtomicPtr::new(ptr::null_mut()), _lifetime_marker: PhantomData }
  }

  /// Initialize the StandardBootServices with a reference to [efi::BootServices].
  /// # Panics
  /// This function will panic if already initialize.
  pub fn initialize(&'a self, efi_boot_services: &'a efi::BootServices) {
    if self.efi_boot_services.load(Ordering::Relaxed).is_null() {
      // The efi::BootServices is only read, that is why we use a non mutable reference.
      self.efi_boot_services.store(efi_boot_services as *const _ as *mut _, Ordering::SeqCst)
    } else {
      panic!("Boot services already initialize.")
    }
  }

  /// # Panics
  /// This function will panic if it was not initialize.
  fn efi_boot_services(&self) -> &efi::BootServices {
    // SAFETY: This pointer is assume to be a valid efi::BootServices pointer since the only way to set it was via an efi::BootServices reference.
    unsafe {
      self.efi_boot_services.load(Ordering::SeqCst).as_ref::<'a>().expect("Boot services has not been initialize.")
    }
  }
}

unsafe impl Sync for StandardBootServices<'_> {}
unsafe impl Send for StandardBootServices<'_> {}

impl BootServices for StandardBootServices<'_> {
  unsafe fn create_event_unchecked<T: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventNotifyCallback<*mut T>>,
    notify_context: *mut T,
  ) -> Result<efi::Event, efi::Status> {
    let mut event = MaybeUninit::zeroed();
    let status = (self.efi_boot_services().create_event)(
      event_type.into(),
      notify_tpl.into(),
      mem::transmute(notify_function),
      notify_context as *mut c_void,
      event.as_mut_ptr(),
    );
    if status.is_error() {
      Err(status)
    } else {
      Ok(event.assume_init())
    }
  }

  unsafe fn create_event_ex_unchecked<T: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: EventNotifyCallback<*mut T>,
    notify_context: *mut T,
    event_group: Option<&'static efi::Guid>,
  ) -> Result<efi::Event, efi::Status> {
    let mut event = MaybeUninit::zeroed();
    let status = (self.efi_boot_services().create_event_ex)(
      event_type.into(),
      notify_tpl.into(),
      mem::transmute(notify_function),
      notify_context as *mut c_void,
      event_group.map_or(ptr::null_mut(), |x| x as *const _),
      event.as_mut_ptr(),
    );
    if status.is_error() {
      Err(status)
    } else {
      Ok(event.assume_init())
    }
  }

  fn close_event(&self, event: efi::Event) -> Result<(), efi::Status> {
    match (self.efi_boot_services().close_event)(event) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn signal_event(&self, event: efi::Event) -> Result<(), efi::Status> {
    match (self.efi_boot_services().signal_event)(event) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn wait_for_event(&self, events: &mut [efi::Event]) -> Result<usize, efi::Status> {
    let mut index = MaybeUninit::zeroed();
    let status = (self.efi_boot_services().wait_for_event)(events.len(), events.as_mut_ptr(), index.as_mut_ptr());
    if status.is_error() {
      Err(status)
    } else {
      Ok(unsafe { index.assume_init() })
    }
  }

  fn check_event(&self, event: efi::Event) -> Result<(), efi::Status> {
    match (self.efi_boot_services().check_event)(event) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn set_timer(&self, event: efi::Event, timer_type: EventTimerType, trigger_time: u64) -> Result<(), efi::Status> {
    match (self.efi_boot_services().set_timer)(event, timer_type.into(), trigger_time) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn raise_tpl(&self, new_tpl: Tpl) -> Tpl {
    (self.efi_boot_services().raise_tpl)(new_tpl.into()).into()
  }

  fn restore_tpl(&self, old_tpl: Tpl) {
    (self.efi_boot_services().restore_tpl)(old_tpl.into())
  }
}

#[cfg(test)]
mod test {
  use efi;

  use super::*;
  use core::mem::MaybeUninit;

  #[test]
  #[should_panic(expected = "Boot services has not been initialize.")]
  fn test_that_accessing_uninit_boot_services_should_panic() {
    let bs = StandardBootServices::new_uninit();
    bs.efi_boot_services();
  }

  #[test]
  #[should_panic(expected = "Boot services already initialize.")]
  fn test_that_initializing_boot_services_multiple_time_should_panic() {
    let efi_bs = unsafe { MaybeUninit::<efi::BootServices>::zeroed().as_ptr().as_ref().unwrap() };
    let bs = StandardBootServices::new_uninit();
    bs.initialize(efi_bs);
    bs.initialize(efi_bs);
  }

  #[test]
  fn test_create_event() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().create_event = efi_create_event;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn notify_callback(_e: efi::Event, ctx: Box<i32>) {
      assert_eq!(10, *ctx)
    }

    extern "efiapi" fn efi_create_event(
      event_type: u32,
      notify_tpl: efi::Tpl,
      notify_function: Option<efi::EventNotify>,
      notify_context: *mut c_void,
      event: *mut efi::Event,
    ) -> efi::Status {
      assert_eq!(efi::EVT_RUNTIME | efi::EVT_NOTIFY_SIGNAL, event_type);
      assert_eq!(efi::TPL_APPLICATION, notify_tpl);
      assert_eq!(notify_callback as *const fn(), unsafe { mem::transmute(notify_function) });
      assert_ne!(ptr::null_mut(), notify_context);
      assert_ne!(ptr::null_mut(), event);

      if let Some(notify_function) = notify_function {
        notify_function(ptr::null_mut(), notify_context);
      }
      efi::Status::SUCCESS
    }

    let ctx = Box::new(10);
    let status = BOOT_SERVICE.create_event(
      EventType::RUNTIME | EventType::NOTIFY_SIGNAL,
      Tpl::APPLICATION,
      Some(notify_callback),
      ctx,
    );

    assert!(matches!(status, Ok(_)));
  }

  #[test]
  fn test_create_event_ex() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().create_event_ex = efi_create_event_ex;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn notify_callback(_e: efi::Event, ctx: Box<i32>) {
      assert_eq!(10, *ctx)
    }

    extern "efiapi" fn efi_create_event_ex(
      event_type: u32,
      notify_tpl: efi::Tpl,
      notify_function: Option<efi::EventNotify>,
      notify_context: *const c_void,
      event_group: *const efi::Guid,
      event: *mut efi::Event,
    ) -> efi::Status {
      assert_eq!(efi::EVT_RUNTIME | efi::EVT_NOTIFY_SIGNAL, event_type);
      assert_eq!(efi::TPL_APPLICATION, notify_tpl);
      assert_eq!(notify_callback as *const fn(), unsafe { mem::transmute(notify_function) });
      assert_ne!(ptr::null(), notify_context);
      assert_ne!(ptr::addr_of!(GUID), event_group);
      assert_ne!(ptr::null_mut(), event);

      if let Some(notify_function) = notify_function {
        notify_function(ptr::null_mut(), notify_context as *mut _);
      }
      efi::Status::SUCCESS
    }
    static GUID: efi::Guid = efi::Guid::from_fields(0, 0, 0, 0, 0, &[0; 6]);
    let ctx = Box::new(10);
    let status = BOOT_SERVICE.create_event_ex(
      EventType::RUNTIME | EventType::NOTIFY_SIGNAL,
      Tpl::APPLICATION,
      Some(notify_callback),
      ctx,
      None,
    );

    assert!(matches!(status, Ok(_)));
  }

  #[test]
  fn test_close_event() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().close_event = efi_close_event;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn efi_close_event(event: efi::Event) -> efi::Status {
      assert_eq!(1, event as usize);
      efi::Status::SUCCESS
    }

    let event = 1_usize as efi::Event;
    let status = BOOT_SERVICE.close_event(event);
    assert!(matches!(status, Ok(())));
  }

  #[test]
  fn test_signal_event() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().signal_event = efi_signal_event;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn efi_signal_event(event: efi::Event) -> efi::Status {
      assert_eq!(1, event as usize);
      efi::Status::SUCCESS
    }

    let event = 1_usize as efi::Event;
    let status = BOOT_SERVICE.signal_event(event);
    assert!(matches!(status, Ok(())));
  }

  #[test]
  fn test_wait_for_event() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().wait_for_event = efi_wait_for_event;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn efi_wait_for_event(
      number_of_event: usize,
      events: *mut efi::Event,
      index: *mut usize,
    ) -> efi::Status {
      assert_eq!(2, number_of_event);
      assert_ne!(ptr::null_mut(), events);

      unsafe { ptr::write(index, 1) }
      efi::Status::SUCCESS
    }

    let mut events = [1_usize as efi::Event, 2_usize as efi::Event];
    let status = BOOT_SERVICE.wait_for_event(&mut events);
    assert!(matches!(status, Ok(1)));
  }

  #[test]
  fn test_check_event() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().check_event = efi_check_event;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn efi_check_event(event: efi::Event) -> efi::Status {
      assert_eq!(1, event as usize);
      efi::Status::SUCCESS
    }

    let event = 1_usize as efi::Event;
    let status = BOOT_SERVICE.check_event(event);
    assert!(matches!(status, Ok(())));
  }

  #[test]
  fn test_set_timer() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().set_timer = efi_set_timer;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn efi_set_timer(event: efi::Event, r#type: efi::TimerDelay, trigger_time: u64) -> efi::Status {
      assert_eq!(1, event as usize);
      assert_eq!(efi::TIMER_PERIODIC, r#type);
      assert_eq!(200, trigger_time);
      efi::Status::SUCCESS
    }

    let event = 1_usize as efi::Event;
    let status = BOOT_SERVICE.set_timer(event, EventTimerType::Periodic, 200);
    assert!(matches!(status, Ok(())));
  }

  #[test]
  fn test_raise_tpl() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().raise_tpl = efi_raise_tpl;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn efi_raise_tpl(tpl: efi::Tpl) -> efi::Tpl {
      assert_eq!(efi::TPL_NOTIFY, tpl);
      efi::TPL_APPLICATION
    }

    let status = BOOT_SERVICE.raise_tpl(Tpl::NOTIFY);
    assert_eq!(Tpl::APPLICATION, status);
  }

  #[test]
  fn test_restore_tpl() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().restore_tpl = efi_restore_tpl;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn efi_restore_tpl(tpl: efi::Tpl) {
      assert_eq!(efi::TPL_APPLICATION, tpl);
    }

    BOOT_SERVICE.restore_tpl(Tpl::APPLICATION);
  }
}
