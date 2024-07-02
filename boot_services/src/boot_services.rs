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
    let mut event = ptr::null_mut();
    let status = (self.efi_boot_services().create_event)(
      event_type.into(),
      notify_tpl.into(),
      mem::transmute(notify_function),
      notify_context as *mut c_void,
      ptr::addr_of_mut!(event),
    );
    if status.is_error() {
      Err(status)
    } else if event.is_null() {
      Err(efi::Status::INVALID_PARAMETER)
    } else {
      Ok(event)
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
  use event::NoContext;
  use std::{os::raw::c_void, sync::atomic::AtomicI32};

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
      _notify_tpl: efi::Tpl,
      notify_function: Option<efi::EventNotify>,
      notify_context: *mut c_void,
      event: *mut efi::Event,
    ) -> efi::Status {
      if let Some(notify_function) = notify_function {
        notify_function(ptr::null_mut(), notify_context);
      }
      unsafe { ptr::write(event, ptr::null_mut()) }
      efi::Status::SUCCESS
    }
    let efi_bs = unsafe { MaybeUninit::<efi::BootServices>::zeroed().as_mut_ptr().as_mut().unwrap() };
    efi_bs.create_event = efi_create_event;
    let bs = StandardBootServices::new_uninit();
    bs.initialize(&efi_bs);

    extern "efiapi" fn foo_ptr(_e: efi::Event, ctx: *mut i32) {
      let ctx = unsafe { ctx.as_ref().unwrap() };
      println!("foo_ptr {:?}", ctx)
    }

    extern "efiapi" fn foo_ref(_e: efi::Event, ctx: &'static AtomicI32) {
      println!("foo_ref {:?}", ctx.load(Ordering::Relaxed))
    }

    extern "efiapi" fn foo_box(_e: efi::Event, ctx: Box<i32>) {
      println!("foo_box {ctx:?}")
      //...
    }

    extern "efiapi" fn foo_box_str(_e: efi::Event, ctx: Box<String>) {
      println!("foo_box {ctx:?}")
      //...
    }

    extern "efiapi" fn foo_box_str_2(_e: efi::Event, ctx: Option<Box<String>>) {
      println!("foo_box {ctx:?}")
      //...
    }

    extern "efiapi" fn foo_box_str_3(_e: efi::Event, ctx: Option<&i32>) {
      println!("foo_box {ctx:?}")
      //...
    }

    extern "efiapi" fn foo_unit(_e: efi::Event, ctx: NoContext) {
      println!("foo_box {ctx:?}")
      //...
    }

    {
      let ctx = Box::new(222222);
      let ctx = Box::into_raw(ctx);
      let _null = unsafe { bs.create_event_unchecked(EventType::RUNTIME, Tpl::APPLICATION, Some(foo_ptr), ctx) };
    }
    {
      static CTX: AtomicI32 = AtomicI32::new(843);
      let _null = bs.create_event(EventType::RUNTIME, Tpl::APPLICATION, Some(foo_ref), &CTX);
    }
    {
      let ctx = Box::new(348379);
      let _null = bs.create_event(EventType::RUNTIME, Tpl::APPLICATION, Some(foo_box), ctx);
    }
    {
      let ctx = Box::new(String::from("value"));
      let _null = bs.create_event(EventType::RUNTIME, Tpl::APPLICATION, Some(foo_box_str), ctx);
    }
    {
      let ctx = Box::new(String::from("value"));
      let _null = bs.create_event(EventType::RUNTIME, Tpl::APPLICATION, Some(foo_box_str_2), Some(ctx));
    }
    {
      static INT: i32 = 0;
      let _null = bs.create_event(EventType::RUNTIME, Tpl::APPLICATION, Some(foo_box_str_3), Some(&INT));
    }
    {
      let _null = bs.create_event(EventType::RUNTIME, Tpl::APPLICATION, Some(foo_unit), ());
    }
  }
}
