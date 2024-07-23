#![cfg_attr(all(not(test), not(feature = "mockall")), no_std)]

#[cfg(feature = "global_allocator")]
pub mod global_allocator;

extern crate alloc;

pub mod allocation;
pub mod boxed;
pub mod event;
pub mod protocol_handler;
pub mod tpl;

#[cfg(any(test, feature = "mockall"))]
use mockall::automock;

use alloc::vec::Vec;
use core::{
  any::Any,
  ffi::c_void,
  marker::PhantomData,
  mem::{self, MaybeUninit},
  option::Option,
  ptr,
  sync::atomic::{AtomicPtr, Ordering},
};

use r_efi::efi;

use allocation::{AllocType, MemoryMap, MemoryType};
use boxed::BootServicesBox;
use event::{EventCtxMutPtr, EventNotifyCallback, EventTimerType, EventType};
use protocol_handler::{HandleSearchType, Protocol, Registration};
use tpl::{Tpl, TplGuard};

/// This is the boot services used in the UEFI.
/// it wraps an atomic ptr to [`efi::BootServices`]
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
      panic!("Boot services is already initialize.")
    }
  }

  /// # Panics
  /// This function will panic if it was not initialize.
  fn efi_boot_services(&self) -> &efi::BootServices {
    // SAFETY: This pointer is assume to be a valid efi::BootServices pointer since the only way to set it was via an efi::BootServices reference.
    unsafe { self.efi_boot_services.load(Ordering::SeqCst).as_ref::<'a>().expect("Boot services is not initialize.") }
  }
}

///SAFETY: StandardBootServices uses an atomic ptr to access the BootServices.
unsafe impl Sync for StandardBootServices<'static> {}
///SAFETY: When the lifetime is `'static`, the pointer is guaranteed to stay valid.
unsafe impl Send for StandardBootServices<'static> {}

/// Functions that are available *before* a successful call to EFI_BOOT_SERVICES.ExitBootServices().
#[cfg_attr(any(test, feature = "mockall"), automock)]
pub trait BootServices: Sized {
  /// Create an event.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-createevent" target="_blank">
  ///   7.1.1. EFI_BOOT_SERVICES.CreateEvent()
  /// </a>
  fn create_event<T: EventCtxMutPtr<Ctx = Ctx> + 'static, Ctx: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventNotifyCallback<T>>,
    notify_context: T,
  ) -> Result<efi::Event, efi::Status> {
    //SAFETY: EventCtxMutPtr generic is used to guaranteed that rust borowing and rules are meet.
    unsafe {
      self.create_event_unchecked(
        event_type,
        notify_tpl,
        mem::transmute(notify_function),
        notify_context.into_raw_mut(),
      )
    }
  }

  /// Prefer normal [`BootServices::create_event`] when possible.
  ///
  /// # Safety
  ///
  /// When calling this method, you have to make sure that *notify_context* pointer is **null** or all of the following is true:
  /// * The pointer must be properly aligned.
  /// * It must be "dereferenceable" into type `T`
  /// * It must remain a valid pointer for the lifetime of the event.
  /// * You must enforce Rust’s borrowing[^borrowing rules] rules rules.
  ///
  /// [^borrowing rules]:
  /// Rust By Example Book:
  /// <a href="https://doc.rust-lang.org/beta/rust-by-example/scope/borrow.html" target="_blank">
  ///   15.3. Borrowing
  /// </a>
  unsafe fn create_event_unchecked<T: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventNotifyCallback<*mut T>>,
    notify_context: *mut T,
  ) -> Result<efi::Event, efi::Status>;

  /// Create an event in a group.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-createeventex" target="_blank">
  ///   7.1.2. EFI_BOOT_SERVICES.CreateEventEx()
  /// </a>
  fn create_event_ex<T: EventCtxMutPtr<Ctx = Ctx> + 'static, Ctx: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: Option<EventNotifyCallback<T>>,
    notify_context: T,
    event_group: &'static efi::Guid,
  ) -> Result<efi::Event, efi::Status> {
    //SAFETY: EventCtxMutPtr generic is used to guaranteed that rust borowing and rules are meet.
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

  /// Prefer normal [`BootServices::create_event_ex`] when possible.
  ///
  /// # Safety
  ///
  /// Make sure to comply to the same constraint as [`BootServices::create_event_unchecked`]
  unsafe fn create_event_ex_unchecked<T: Sized + 'static>(
    &self,
    event_type: EventType,
    notify_tpl: Tpl,
    notify_function: EventNotifyCallback<*mut T>,
    notify_context: *mut T,
    event_group: &'static efi::Guid,
  ) -> Result<efi::Event, efi::Status>;

  /// Close an event.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-closeevent" target="_blank">
  ///   7.1.3. EFI_BOOT_SERVICES.CloseEvent()
  /// </a>
  ///
  /// [^note]: It is safe to call *close_event* in the notify function.
  fn close_event(&self, event: efi::Event) -> Result<(), efi::Status>;

  /// Signals an event.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-signalevent" target="_blank">
  ///   7.1.4. EFI_BOOT_SERVICES.SignalEvent()
  /// </a>
  fn signal_event(&self, event: efi::Event) -> Result<(), efi::Status>;

  /// Stops execution until an event is signaled.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-waitforevent" target="_blank">
  ///   7.1.5. EFI_BOOT_SERVICES.WaitForEvent()
  /// </a>
  fn wait_for_event(&self, events: &mut [efi::Event]) -> Result<usize, efi::Status>;

  /// Checks whether an event is in the signaled state.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-checkevent" target="_blank">
  ///   7.1.6. EFI_BOOT_SERVICES.CheckEvent()
  /// </a>
  fn check_event(&self, event: efi::Event) -> Result<(), efi::Status>;

  /// Sets the type of timer and the trigger time for a timer event.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-settimer" target="_blank">
  ///   7.1.7. EFI_BOOT_SERVICES.SetTimer()
  /// </a>
  fn set_timer(&self, event: efi::Event, timer_type: EventTimerType, trigger_time: u64) -> Result<(), efi::Status>;

  /// Raises a task's priority level and returns a [`TplGuard`] that will restore the tpl when dropped.
  ///
  /// See [`BootServices::raise_tpl`] and [`BootServices::restore_tpl`] for more details.
  fn raise_tpl_guarded<'a>(&'a self, tpl: Tpl) -> TplGuard<'a, Self> {
    TplGuard { boot_services: self, retore_tpl: self.raise_tpl(tpl) }
  }

  /// Raises a task’s priority level and returns its previous level.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-raisetpl" target="_blank">
  ///   7.1.8. EFI_BOOT_SERVICES.RaiseTPL()
  /// </a>
  fn raise_tpl(&self, tpl: Tpl) -> Tpl;

  /// Restores a task’s priority level to its previous value.
  ///
  /// UEFI Spec Documentation:
  /// <a href="https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-restoretpl" target="_blank">
  ///   7.1.9. EFI_BOOT_SERVICES.RestoreTPL()
  /// </a>
  fn restore_tpl(&self, tpl: Tpl);

  fn allocate_pages(
    &self,
    alloc_type: AllocType,
    memory_type: MemoryType,
    nb_pages: usize,
  ) -> Result<usize, efi::Status>;

  fn free_pages(&self, address: usize, nb_pages: usize) -> Result<(), efi::Status>;

  fn get_memory_map<'a>(&'a self) -> Result<MemoryMap<'a, Self>, (efi::Status, usize)>;

  fn allocate_pool(&self, pool_type: MemoryType, size: usize) -> Result<*mut u8, efi::Status>;

  fn free_pool(&self, buffer: *mut u8) -> Result<(), efi::Status>;

  fn install_protocol_interface<P: Protocol<Interface = I> + 'static, I: Any + 'static>(
    &self,
    handle: Option<efi::Handle>,
    protocol: &P,
    interface: &'static mut I,
  ) -> Result<efi::Handle, efi::Status> {
    let interface_any = interface as &dyn Any;
    let interface_ptr = match interface_any.downcast_ref::<()>() {
      Some(()) => ptr::null_mut(),
      None => interface as *mut _ as *mut c_void,
    };
    //SAFETY: The generic Protocol ensure that the interface is the right type for the specified protocol.
    unsafe { self.install_protocol_interface_unchecked(handle, protocol.protocol_guid(), interface_ptr) }
  }

  unsafe fn install_protocol_interface_unchecked(
    &self,
    handle: Option<efi::Handle>,
    protocol: &'static efi::Guid,
    interface: *mut c_void,
  ) -> Result<efi::Handle, efi::Status>;

  fn uninstall_protocol_interface<P: Protocol<Interface = I> + 'static, I: 'static>(
    &self,
    handle: efi::Handle,
    protocol: &P,
    interface: Option<&'static mut I>,
  ) -> Result<(), efi::Status> {
    //SAFETY: The generic Protocol ensure that the interface is the right type for the specified protocol.
    unsafe {
      self.uninstall_protocol_interface_unchecked(
        handle,
        protocol.protocol_guid(),
        interface.map(|i| i as *mut _ as *mut c_void),
      )
    }
  }

  unsafe fn uninstall_protocol_interface_unchecked(
    &self,
    handle: efi::Handle,
    protocol: &'static efi::Guid,
    interface: Option<*mut c_void>,
  ) -> Result<(), efi::Status>;

  fn reinstall_protocol_interface<P: Protocol<Interface = I> + 'static, I: 'static>(
    &self,
    handle: efi::Handle,
    protocol: &P,
    old_protocol_interface: Option<&'static mut I>,
    new_protocol_interface: Option<&'static mut I>,
  ) -> Result<(), efi::Status> {
    //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
    unsafe {
      self.reinstall_protocol_interface_unchecked(
        handle,
        protocol.protocol_guid(),
        old_protocol_interface.map(|i| i as *mut _ as *mut c_void),
        new_protocol_interface.map(|i| i as *mut _ as *mut c_void),
      )
    }
  }

  unsafe fn reinstall_protocol_interface_unchecked(
    &self,
    handle: efi::Handle,
    protocol: &'static efi::Guid,
    old_protocol_interface: Option<*mut c_void>,
    new_protocol_interface: Option<*mut c_void>,
  ) -> Result<(), efi::Status>;

  fn register_protocol_notify(
    &self,
    protocol: &'static efi::Guid,
    event: efi::Event,
  ) -> Result<Registration, efi::Status>;

  fn locate_handle<'a>(
    &'a self,
    search_type: HandleSearchType,
  ) -> Result<BootServicesBox<'a, [efi::Handle], Self>, efi::Status>;

  fn handle_protocol<P: Protocol<Interface = I> + 'static, I: 'static>(
    &self,
    handle: efi::Handle,
    protocol: &P,
  ) -> Result<Option<&'static mut I>, efi::Status> {
    //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
    unsafe { self.handle_protocol_unchecked(handle, protocol.protocol_guid()).map(|i| (i as *mut I).as_mut()) }
  }

  fn handle_protocol_unchecked(&self, handle: efi::Handle, protocol: &efi::Guid) -> Result<*mut c_void, efi::Status>;

  unsafe fn locate_device_path(
    &self,
    protocol: &efi::Guid,
    device_path: *mut *mut efi::protocols::device_path::Protocol,
  ) -> Result<efi::Handle, efi::Status>;

  fn open_protocol<P: Protocol<Interface = I> + 'static, I: 'static>(
    &self,
    handle: efi::Handle,
    protocol: &P,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
    attribute: u32,
  ) -> Result<Option<&'static mut I>, efi::Status> {
    //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
    unsafe {
      self
        .open_protocol_unchecked(handle, protocol, agent_handle, controller_handle, attribute)
        .map(|i| (i as *mut I).as_mut())
    }
  }

  fn open_protocol_unchecked(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
    attribute: u32,
  ) -> Result<*mut c_void, efi::Status>;

  fn close_protocol(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
  ) -> Result<(), efi::Status>;

  fn open_protocol_information<'a>(
    &'a self,
    handle: efi::Handle,
    protocol: &efi::Guid,
  ) -> Result<BootServicesBox<'a, [efi::OpenProtocolInformationEntry], Self>, efi::Status>;

  unsafe fn connect_controller(
    &self,
    controller_handle: efi::Handle,
    driver_image_handle: Vec<efi::Handle>,
    remaining_device_path: Option<*mut efi::protocols::device_path::Protocol>,
    recursive: bool,
  ) -> Result<(), efi::Status>;

  fn disconnect_controller(
    &self,
    controller_handle: efi::Handle,
    driver_image_handle: Option<efi::Handle>,
    child_handle: Option<efi::Handle>,
  ) -> Result<(), efi::Status>;

  fn protocols_per_handle<'a>(
    &'a self,
    handle: efi::Handle,
  ) -> Result<BootServicesBox<'a, [efi::Guid], Self>, efi::Status>;

  fn locate_handle_buffer<'a>(
    &'a self,
    search_type: HandleSearchType,
  ) -> Result<BootServicesBox<'a, [efi::Handle], Self>, efi::Status>;

  fn locate_protocol<P: Protocol<Interface = I> + 'static, I: 'static>(
    &self,
    protocol: &P,
    registration: Option<Registration>,
  ) -> Option<&'static mut I> {
    //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
    unsafe { self.locate_protocol_unchecked(protocol.protocol_guid(), registration).map(|x| (x as *mut I).as_mut()) }
      .unwrap_or(None)
  }

  fn locate_protocol_unchecked(
    &self,
    protocol: &'static efi::Guid,
    registration: Option<*mut c_void>,
  ) -> Result<*mut c_void, efi::Status>;
}

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
    event_group: &'static efi::Guid,
  ) -> Result<efi::Event, efi::Status> {
    let mut event = MaybeUninit::zeroed();
    let status = (self.efi_boot_services().create_event_ex)(
      event_type.into(),
      notify_tpl.into(),
      mem::transmute(notify_function),
      notify_context as *mut c_void,
      event_group as *const _,
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

  fn allocate_pages(
    &self,
    alloc_type: AllocType,
    memory_type: MemoryType,
    nb_pages: usize,
  ) -> Result<usize, efi::Status> {
    let mut memory_address = match alloc_type {
      AllocType::Address(address) => address,
      _ => 0,
    };
    match (self.efi_boot_services().allocate_pages)(
      alloc_type.into(),
      memory_type.into(),
      nb_pages,
      ptr::addr_of_mut!(memory_address) as *mut u64,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(memory_address),
    }
  }

  fn free_pages(&self, address: usize, nb_pages: usize) -> Result<(), efi::Status> {
    match (self.efi_boot_services().free_pages)(address as u64, nb_pages) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn get_memory_map<'a>(&'a self) -> Result<MemoryMap<'a, Self>, (efi::Status, usize)> {
    let mut memory_map_size = 0;
    let mut map_key = 0;
    let mut descriptor_size = 0;
    let mut descriptor_version = 0;

    match (self.efi_boot_services().get_memory_map)(
      ptr::addr_of_mut!(memory_map_size),
      ptr::null_mut(),
      ptr::addr_of_mut!(map_key),
      ptr::addr_of_mut!(descriptor_size),
      ptr::addr_of_mut!(descriptor_version),
    ) {
      s if s == efi::Status::BUFFER_TOO_SMALL => memory_map_size += 64, // add more space in case allocation makes the memory map bigger.
      _ => (),
    };

    let buffer = self.allocate_pool(MemoryType::BootServicesData, memory_map_size).map_err(|s| (s, 0))?;

    match (self.efi_boot_services().get_memory_map)(
      ptr::addr_of_mut!(memory_map_size),
      buffer as *mut _,
      ptr::addr_of_mut!(map_key),
      ptr::addr_of_mut!(descriptor_size),
      ptr::addr_of_mut!(descriptor_version),
    ) {
      s if s == efi::Status::BUFFER_TOO_SMALL => return Err((s, memory_map_size)),
      s if s.is_error() => return Err((s, 0)),
      _ => (),
    }
    Ok(MemoryMap {
      descriptors: unsafe { BootServicesBox::from_raw_parts(buffer as *mut _, descriptor_size, self) },
      map_key,
      descriptor_version,
    })
  }

  fn allocate_pool(&self, memory_type: MemoryType, size: usize) -> Result<*mut u8, efi::Status> {
    let mut buffer = ptr::null_mut();
    match (self.efi_boot_services().allocate_pool)(memory_type.into(), size, ptr::addr_of_mut!(buffer)) {
      s if s.is_error() => return Err(s),
      _ => Ok(buffer as *mut u8),
    }
  }

  fn free_pool(&self, buffer: *mut u8) -> Result<(), efi::Status> {
    match (self.efi_boot_services().free_pool)(buffer as *mut c_void) {
      s if s.is_error() => return Err(s),
      _ => Ok(()),
    }
  }

  unsafe fn install_protocol_interface_unchecked(
    &self,
    handle: Option<efi::Handle>,
    protocol: &'static efi::Guid,
    interface: *mut c_void,
  ) -> Result<efi::Handle, efi::Status> {
    let mut handle = handle.unwrap_or(ptr::null_mut());
    match (self.efi_boot_services().install_protocol_interface)(
      ptr::addr_of_mut!(handle),
      protocol as *const _ as *mut _,
      efi::NATIVE_INTERFACE,
      interface,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(handle),
    }
  }

  unsafe fn uninstall_protocol_interface_unchecked(
    &self,
    handle: efi::Handle,
    protocol: &'static efi::Guid,
    interface: Option<*mut c_void>,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().uninstall_protocol_interface)(
      handle,
      protocol as *const _ as *mut _,
      interface.unwrap_or(ptr::null_mut()),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  unsafe fn reinstall_protocol_interface_unchecked(
    &self,
    handle: efi::Handle,
    protocol: &'static efi::Guid,
    old_protocol_interface: Option<*mut c_void>,
    new_protocol_interface: Option<*mut c_void>,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().reinstall_protocol_interface)(
      handle,
      protocol as *const _ as *mut _,
      old_protocol_interface.unwrap_or(ptr::null_mut()),
      new_protocol_interface.unwrap_or(ptr::null_mut()),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn register_protocol_notify(&self, protocol: &efi::Guid, event: efi::Event) -> Result<Registration, efi::Status> {
    let mut registration = MaybeUninit::uninit();
    match (self.efi_boot_services().register_protocol_notify)(
      protocol as *const _ as *mut _,
      event,
      registration.as_mut_ptr() as *mut _,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(unsafe { registration.assume_init() }),
    }
  }

  fn locate_handle(&self, search_type: HandleSearchType) -> Result<BootServicesBox<[efi::Handle], Self>, efi::Status> {
    let protocol = match search_type {
      HandleSearchType::ByProtocol(p) => p as *const _ as *mut _,
      _ => ptr::null_mut(),
    };
    let search_key = match search_type {
      HandleSearchType::ByRegisterNotify(k) => k,
      _ => ptr::null_mut(),
    };

    // Use to get the buffer_size
    let mut buffer_size = 0;
    (self.efi_boot_services().locate_handle)(
      search_type.into(),
      protocol,
      search_key,
      ptr::addr_of_mut!(buffer_size),
      ptr::null_mut(),
    );

    let buffer = self.allocate_pool(MemoryType::BootServicesData, buffer_size)?;

    match (self.efi_boot_services().locate_handle)(
      search_type.into(),
      protocol,
      search_key,
      ptr::addr_of_mut!(buffer_size),
      buffer as *mut efi::Handle,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(unsafe {
        BootServicesBox::from_raw_parts(buffer as *mut _, buffer_size / mem::size_of::<efi::Handle>(), &self)
      }),
    }
  }

  fn handle_protocol_unchecked(&self, handle: efi::Handle, protocol: &efi::Guid) -> Result<*mut c_void, efi::Status> {
    let mut interface = ptr::null_mut();
    match (self.efi_boot_services().handle_protocol)(
      handle,
      protocol as *const _ as *mut _,
      ptr::addr_of_mut!(interface),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(interface),
    }
  }

  unsafe fn locate_device_path(
    &self,
    protocol: &efi::Guid,
    device_path: *mut *mut efi::protocols::device_path::Protocol,
  ) -> Result<efi::Handle, efi::Status> {
    let mut device = ptr::null_mut();
    match (self.efi_boot_services().locate_device_path)(
      protocol as *const _ as *mut _,
      device_path,
      ptr::addr_of_mut!(device),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(device),
    }
  }

  fn open_protocol_unchecked(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
    attribute: u32,
  ) -> Result<*mut c_void, efi::Status> {
    let mut interface = ptr::null_mut();
    match (self.efi_boot_services().open_protocol)(
      handle,
      protocol as *const _ as *mut _,
      ptr::addr_of_mut!(interface),
      agent_handle,
      controller_handle,
      attribute,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(interface),
    }
  }

  fn close_protocol(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
    agent_handle: efi::Handle,
    controller_handle: efi::Handle,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().close_protocol)(
      handle,
      protocol as *const _ as *mut _,
      agent_handle,
      controller_handle,
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn open_protocol_information(
    &self,
    handle: efi::Handle,
    protocol: &efi::Guid,
  ) -> Result<BootServicesBox<[efi::OpenProtocolInformationEntry], Self>, efi::Status>
  where
    Self: Sized,
  {
    let mut entry_buffer = ptr::null_mut();
    let mut entry_count = 0;
    match (self.efi_boot_services().open_protocol_information)(
      handle,
      protocol as *const _ as *mut _,
      ptr::addr_of_mut!(entry_buffer),
      ptr::addr_of_mut!(entry_count),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(unsafe { BootServicesBox::from_raw_parts(entry_buffer, entry_count, self) }),
    }
  }

  unsafe fn connect_controller(
    &self,
    controller_handle: efi::Handle,
    mut driver_image_handle: Vec<efi::Handle>,
    remaining_device_path: Option<*mut efi::protocols::device_path::Protocol>,
    recursive: bool,
  ) -> Result<(), efi::Status> {
    let driver_image_handle = if driver_image_handle.is_empty() {
      ptr::null_mut()
    } else {
      driver_image_handle.push(ptr::null_mut());
      driver_image_handle.as_mut_ptr()
    };
    match (self.efi_boot_services().connect_controller)(
      controller_handle,
      driver_image_handle,
      remaining_device_path.unwrap_or(ptr::null_mut()),
      recursive.into(),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn disconnect_controller(
    &self,
    controller_handle: efi::Handle,
    driver_image_handle: Option<efi::Handle>,
    child_handle: Option<efi::Handle>,
  ) -> Result<(), efi::Status> {
    match (self.efi_boot_services().disconnect_controller)(
      controller_handle,
      driver_image_handle.unwrap_or(ptr::null_mut()),
      child_handle.unwrap_or(ptr::null_mut()),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(()),
    }
  }

  fn protocols_per_handle(&self, handle: efi::Handle) -> Result<BootServicesBox<[efi::Guid], Self>, efi::Status> {
    let mut protocol_buffer = ptr::null_mut();
    let mut protocol_buffer_count = 0;
    match (self.efi_boot_services().protocols_per_handle)(
      handle,
      ptr::addr_of_mut!(protocol_buffer),
      ptr::addr_of_mut!(protocol_buffer_count),
    ) {
      s if s.is_error() => Err(s),
      _ => {
        Ok(unsafe { BootServicesBox::<[_], _>::from_raw_parts(protocol_buffer as *mut _, protocol_buffer_count, self) })
      }
    }
  }

  fn locate_handle_buffer(
    &self,
    search_type: HandleSearchType,
  ) -> Result<BootServicesBox<[efi::Handle], Self>, efi::Status>
  where
    Self: Sized,
  {
    let mut buffer = ptr::null_mut();
    let mut buffer_count = 0;
    let protocol = match search_type {
      HandleSearchType::ByProtocol(p) => p as *const _ as *mut _,
      _ => ptr::null_mut(),
    };
    let search_key = match search_type {
      HandleSearchType::ByRegisterNotify(k) => k,
      _ => ptr::null_mut(),
    };
    match (self.efi_boot_services().locate_handle_buffer)(
      search_type.into(),
      protocol,
      search_key,
      ptr::addr_of_mut!(buffer_count),
      ptr::addr_of_mut!(buffer),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(unsafe { BootServicesBox::<[_], _>::from_raw_parts(buffer as *mut efi::Handle, buffer_count, self) }),
    }
  }

  fn locate_protocol_unchecked(
    &self,
    protocol: &'static efi::Guid,
    registration: Option<*mut c_void>,
  ) -> Result<*mut c_void, efi::Status> {
    let mut interface = ptr::null_mut();
    match (self.efi_boot_services().locate_protocol)(
      protocol as *const _ as *mut _,
      registration.unwrap_or(ptr::null_mut()),
      ptr::addr_of_mut!(interface),
    ) {
      s if s.is_error() => Err(s),
      _ => Ok(interface),
    }
  }
}

#[cfg(test)]
mod test {
  use efi;

  use super::*;
  use core::{mem::MaybeUninit, sync::atomic::AtomicUsize};

  #[test]
  #[should_panic(expected = "Boot services is not initialize.")]
  fn test_that_accessing_uninit_boot_services_should_panic() {
    let bs = StandardBootServices::new_uninit();
    bs.efi_boot_services();
  }

  #[test]
  #[should_panic(expected = "Boot services is already initialize.")]
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
  fn test_create_event_no_notify() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().create_event = efi_create_event;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    extern "efiapi" fn efi_create_event(
      event_type: u32,
      notify_tpl: efi::Tpl,
      notify_function: Option<efi::EventNotify>,
      notify_context: *mut c_void,
      event: *mut efi::Event,
    ) -> efi::Status {
      assert_eq!(efi::EVT_RUNTIME | efi::EVT_NOTIFY_SIGNAL, event_type);
      assert_eq!(efi::TPL_APPLICATION, notify_tpl);
      assert_eq!(None, notify_function);
      assert_ne!(ptr::null_mut(), notify_context);
      assert_ne!(ptr::null_mut(), event);
      efi::Status::SUCCESS
    }

    let status = BOOT_SERVICE.create_event(EventType::RUNTIME | EventType::NOTIFY_SIGNAL, Tpl::APPLICATION, None, &());

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
      assert_eq!(ptr::addr_of!(GUID), event_group);
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
      &GUID,
    );

    assert!(matches!(status, Ok(_)));
  }

  #[test]
  fn test_create_event_ex_no_notify() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().create_event_ex = efi_create_event_ex;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

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
      assert_eq!(None, notify_function);
      assert_ne!(ptr::null(), notify_context);
      assert_eq!(ptr::addr_of!(GUID), event_group);
      assert_ne!(ptr::null_mut(), event);
      efi::Status::SUCCESS
    }
    static GUID: efi::Guid = efi::Guid::from_fields(0, 0, 0, 0, 0, &[0; 6]);
    let status =
      BOOT_SERVICE.create_event_ex(EventType::RUNTIME | EventType::NOTIFY_SIGNAL, Tpl::APPLICATION, None, &(), &GUID);

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
  fn test_raise_tpl_guarded() {
    static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
    let efi_boot_services = unsafe {
      let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
      bs.assume_init_mut().raise_tpl = efi_raise_tpl;
      bs.assume_init_mut().restore_tpl = efi_restore_tpl;
      bs.assume_init()
    };
    BOOT_SERVICE.initialize(&efi_boot_services);

    static CURRENT_TPL: AtomicUsize = AtomicUsize::new(efi::TPL_APPLICATION);

    extern "efiapi" fn efi_raise_tpl(tpl: efi::Tpl) -> efi::Tpl {
      assert_eq!(efi::TPL_NOTIFY, tpl);
      CURRENT_TPL.swap(tpl, Ordering::Relaxed)
    }

    extern "efiapi" fn efi_restore_tpl(tpl: efi::Tpl) {
      assert_eq!(efi::TPL_APPLICATION, tpl);
      CURRENT_TPL.swap(tpl, Ordering::Relaxed);
    }

    let guard = BOOT_SERVICE.raise_tpl_guarded(Tpl::NOTIFY);
    assert_eq!(Tpl::APPLICATION, guard.retore_tpl);
    assert_eq!(efi::TPL_NOTIFY, CURRENT_TPL.load(Ordering::Relaxed));
    drop(guard);
    assert_eq!(efi::TPL_APPLICATION, CURRENT_TPL.load(Ordering::Relaxed));
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
