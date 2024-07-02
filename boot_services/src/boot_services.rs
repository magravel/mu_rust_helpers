pub mod event;
pub mod tpl;

#[cfg(any(test, feature = "mockall"))]
use mockall::automock;

use core::{
  ffi::c_void,
  marker::PhantomData,
  mem,
  option::Option,
  ptr,
  sync::atomic::{AtomicPtr, Ordering},
};
use std::mem::MaybeUninit;

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
    let event = ptr::null_mut();
    let status = (self.efi_boot_services().create_event_ex)(
      event_type.into(),
      notify_tpl.into(),
      mem::transmute(notify_function),
      notify_context as *mut c_void,
      event_group.map(|g| g as *const _).unwrap_or(ptr::null()),
      event,
    );
    if status.is_error() {
      Err(status)
    } else if event.is_null() {
      Err(efi::Status::INVALID_PARAMETER)
    } else {
      Ok(*event)
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
    let index = ptr::null_mut();
    let status = (self.efi_boot_services().wait_for_event)(events.len(), events.as_mut_ptr(), index);
    if status.is_error() {
      Err(status)
    } else if index.is_null() {
      Err(efi::Status::INVALID_PARAMETER)
    } else {
      Ok(unsafe { *index })
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
}
