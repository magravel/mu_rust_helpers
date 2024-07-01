pub mod event;

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

use r_efi::efi::{self, Event, EventNotify, Guid, TimerDelay, Tpl};

use event::{EventCallback, EventCtxMutPtr, EventType};

#[cfg_attr(any(test, feature = "mockall"), automock)]
pub trait BootServices {
  fn create_event<T: Sized + 'static, R: EventCtxMutPtr<T, FFIType = C> + 'static, C: 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventCallback<C>>,
    notify_context: R,
  ) -> Result<Event, efi::Status> {
    unsafe {
      self.create_event_unchecked::<C>(
        event_type,
        notify_tpl,
        mem::transmute(notify_function),
        mem::transmute(notify_context.into_raw_mut()),
      )
    }
  }

  unsafe fn create_event_unchecked<T: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventCallback<*mut T>>,
    notify_context: *mut T,
  ) -> Result<Event, efi::Status>;

  fn create_event_ex(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: EventNotify,
    notify_context: *mut c_void,
    event_group: *const Guid,
  ) -> Result<Event, efi::Status>;

  fn close_event(&self, event: Event) -> Result<(), efi::Status>;

  fn signal_event(&self, event: Event) -> Result<(), efi::Status>;

  fn wait_for_event(&self, events: &[Event]) -> Result<usize, efi::Status>;

  fn check_event(&self, event: Event) -> Result<(), efi::Status>;

  fn set_timer(&self, event: Event, timer_type: TimerDelay, trigger_time: u64) -> Result<(), efi::Status>;

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
    notify_function: Option<extern "efiapi" fn(Event, *mut T)>,
    notify_context: *mut T,
  ) -> Result<Event, efi::Status> {
    let event = ptr::null_mut();
    let status = (self.efi_boot_services().create_event)(
      event_type.into(),
      notify_tpl,
      mem::transmute(notify_function),
      notify_context as *mut c_void,
      event,
    );
    if status.is_error() {
      Err(status)?;
    }
    if event.is_null() {

    }
    if event == ptr::null_mut() {
      Err(efi::Status::INVALID_PARAMETER)?;
    }
    Ok(*event)
  }

  fn create_event_ex(
    &self,
    _event_type: EventType,
    _notify_tpl: Tpl,
    _notify_function: EventNotify,
    _notify_context: *mut c_void,
    _event_group: *const Guid,
  ) -> Result<Event, efi::Status> {
    todo!()
  }

  fn close_event(&self, _event: Event) -> Result<(), efi::Status> {
    todo!()
  }

  fn signal_event(&self, _event: Event) -> Result<(), efi::Status> {
    todo!()
  }

  fn wait_for_event(&self, _events: &[Event]) -> Result<usize, efi::Status> {
    todo!()
  }

  fn check_event(&self, _event: Event) -> Result<(), efi::Status> {
    todo!()
  }

  fn set_timer(&self, _event: Event, _timer_type: TimerDelay, _trigger_tiem: u64) -> Result<(), efi::Status> {
    todo!()
  }

  fn raise_tpl(&self, new_tpl: efi::Tpl) -> efi::Tpl {
    (self.efi_boot_services().raise_tpl)(new_tpl)
  }

  fn restore_tpl(&self, old_tpl: efi::Tpl) {
    (self.efi_boot_services().restore_tpl)(old_tpl)
  }
}

#[cfg(test)]
mod test {
  use core::mem::MaybeUninit;
  use std::{os::raw::c_void, sync::atomic::AtomicI32};

  use efi::TPL_APPLICATION;

  use super::*;
  use event::NoContext;

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
  fn t() {
    extern "efiapi" fn efi_create_event(
      _event_type: u32,
      _notify_tpl: Tpl,
      notify_function: Option<EventNotify>,
      notify_context: *mut c_void,
      _event: *mut Event,
    ) -> efi::Status {
      if let Some(notify_function) = notify_function {
        notify_function(ptr::null_mut(), notify_context);
      }
      efi::Status::SUCCESS
    }
    let efi_bs = unsafe { MaybeUninit::<efi::BootServices>::zeroed().as_mut_ptr().as_mut().unwrap() };
    efi_bs.create_event = efi_create_event;
    let bs = StandardBootServices::new_uninit();
    bs.initialize(&efi_bs);

    extern "efiapi" fn foo_ptr(_e: Event, ctx: *mut i32) {
      let ctx = unsafe {
          ctx.as_ref().unwrap()
      };
      println!("foo_ptr {:?}", ctx)
    }

    extern "efiapi" fn foo_ref(_e: Event, ctx: &'static AtomicI32) {
      println!("foo_ref {:?}", ctx.load(Ordering::Relaxed))
    }

    extern "efiapi" fn foo_box(_e: Event, ctx: Box<i32>) {
      println!("foo_box {ctx:?}")
      //...
    }

    extern "efiapi" fn foo_box_str(_e: Event, ctx: Box<String>) {
      println!("foo_box {ctx:?}")
      //...
    }

    extern "efiapi" fn foo_unit(_e: Event, ctx: NoContext) {
      println!("foo_box {ctx:?}")
      //...
    }

    {
      let ctx = Box::new(222222);
      let ctx = Box::into_raw(ctx);
      let _null = unsafe { bs.create_event_unchecked(EventType::RUNTIME, TPL_APPLICATION, Some(foo_ptr), ctx) };
    }
    {
      static  CTX: AtomicI32 = AtomicI32::new(843);
      let _null = bs.create_event(EventType::RUNTIME, TPL_APPLICATION, Some(foo_ref), &CTX);
    }
    {
      let ctx = Box::new(348379);
      let _null = bs.create_event(EventType::RUNTIME, TPL_APPLICATION, Some(foo_box), ctx);
    }
    {
      let ctx = Box::new(String::from("value"));
      let _null = bs.create_event(EventType::RUNTIME, TPL_APPLICATION, Some(foo_box_str), ctx);
    }
    {
      let _null = bs.create_event(EventType::RUNTIME, TPL_APPLICATION, Some(foo_unit), ());
    }
  }
}
