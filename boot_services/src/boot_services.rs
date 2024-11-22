#![cfg_attr(all(not(test), not(feature = "mockall")), no_std)]

#[cfg(feature = "global_allocator")]
pub mod global_allocator;

extern crate alloc;

pub mod allocation;
pub mod boxed;
pub mod event;
pub mod ffi_helper;
pub mod protocol_handler;
pub mod static_ptr;
pub mod tpl;

#[cfg(any(test, feature = "mockall"))]
use mockall::automock;

use alloc::vec::Vec;
use core::{
    any::{Any, TypeId},
    ffi::c_void,
    marker::PhantomData,
    mem::{self, MaybeUninit},
    option::Option,
    ptr::{self, NonNull},
    sync::atomic::{AtomicPtr, Ordering},
};
use ffi_helper::{CMutRef, PtrMetadata};
use static_ptr::{StaticPtr, StaticPtrMut};

use r_efi::efi;

use allocation::{AllocType, MemoryMap, MemoryType};
use boxed::BootServicesBox;
use event::{EventNotifyCallback, EventTimerType, EventType};
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
        Self {
            efi_boot_services: AtomicPtr::new(efi_boot_services as *const _ as *mut _),
            _lifetime_marker: PhantomData,
        }
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
        unsafe {
            self.efi_boot_services.load(Ordering::SeqCst).as_ref::<'a>().expect("Boot services is not initialize.")
        }
    }
}

///SAFETY: StandardBootServices uses an atomic ptr to access the BootServices.
unsafe impl Sync for StandardBootServices<'static> {}
///SAFETY: When the lifetime is `'static`, the pointer is guaranteed to stay valid.
unsafe impl Send for StandardBootServices<'static> {}

/// Functions that are available *before* a successful call to EFI_BOOT_SERVICES.ExitBootServices().
#[cfg_attr(any(test, feature = "mockall"), automock)]
pub trait BootServices {
    /// Create an event.
    ///
    /// [UEFI Spec Documentation: 7.1.1. EFI_BOOT_SERVICES.CreateEvent()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-createevent)
    fn create_event<T>(
        &self,
        event_type: EventType,
        notify_tpl: Tpl,
        notify_function: Option<EventNotifyCallback<T>>,
        notify_context: T,
    ) -> Result<efi::Event, efi::Status>
    where
        T: StaticPtr + 'static,
        <T as StaticPtr>::Pointee: Sized + 'static,
    {
        //SAFETY: ['StaticPtr`] generic is used to guaranteed that rust borowing and rules are meet.
        unsafe {
            self.create_event_unchecked(
                event_type,
                notify_tpl,
                mem::transmute(notify_function),
                notify_context.into_raw() as *mut <T as StaticPtr>::Pointee,
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
    /// [Rust By Example Book: 15.3. Borrowing](https://doc.rust-lang.org/beta/rust-by-example/scope/borrow.html)
    unsafe fn create_event_unchecked<T: Sized + 'static>(
        &self,
        event_type: EventType,
        notify_tpl: Tpl,
        notify_function: Option<EventNotifyCallback<*mut T>>,
        notify_context: *mut T,
    ) -> Result<efi::Event, efi::Status>;

    /// Create an event in a group.
    ///
    /// [UEFI Spec Documentation: 7.1.2. EFI_BOOT_SERVICES.CreateEventEx()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-createeventex)
    fn create_event_ex<T>(
        &self,
        event_type: EventType,
        notify_tpl: Tpl,
        notify_function: Option<EventNotifyCallback<T>>,
        notify_context: T,
        event_group: &'static efi::Guid,
    ) -> Result<efi::Event, efi::Status>
    where
        T: StaticPtr + 'static,
        <T as StaticPtr>::Pointee: Sized + 'static,
    {
        //SAFETY: [`StaticPtr`] generic is used to guaranteed that rust borowing and rules are meet.
        unsafe {
            self.create_event_ex_unchecked(
                event_type,
                notify_tpl.into(),
                mem::transmute(notify_function),
                notify_context.into_raw() as *mut <T as StaticPtr>::Pointee,
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
    /// [UEFI Spec Documentation: 7.1.3. EFI_BOOT_SERVICES.CloseEvent()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-closeevent)
    ///
    /// [^note]: It is safe to call *close_event* in the notify function.
    fn close_event(&self, event: efi::Event) -> Result<(), efi::Status>;

    /// Signals an event.
    ///
    /// [UEFI Spec Documentation: 7.1.4. EFI_BOOT_SERVICES.SignalEvent()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-signalevent)
    fn signal_event(&self, event: efi::Event) -> Result<(), efi::Status>;

    /// Stops execution until an event is signaled.
    ///
    /// [UEFI Spec Documentation: 7.1.5. EFI_BOOT_SERVICES.WaitForEvent()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-waitforevent)
    fn wait_for_event(&self, events: &mut [efi::Event]) -> Result<usize, efi::Status>;

    /// Checks whether an event is in the signaled state.
    ///
    /// [UEFI Spec Documentation: 7.1.6. EFI_BOOT_SERVICES.CheckEvent()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-checkevent)
    fn check_event(&self, event: efi::Event) -> Result<(), efi::Status>;

    /// Sets the type of timer and the trigger time for a timer event.
    ///
    /// [UEFI Spec Documentation: 7.1.7. EFI_BOOT_SERVICES.SetTimer()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-settimer)
    fn set_timer(&self, event: efi::Event, timer_type: EventTimerType, trigger_time: u64) -> Result<(), efi::Status>;

    /// Raises a task's priority level and returns a [`TplGuard`] that will restore the tpl when dropped.
    ///
    /// See [`BootServices::raise_tpl`] and [`BootServices::restore_tpl`] for more details.
    fn raise_tpl_guarded<'a>(&'a self, tpl: Tpl) -> TplGuard<'a, Self> {
        TplGuard { boot_services: self, retore_tpl: self.raise_tpl(tpl) }
    }

    /// Raises a task’s priority level and returns its previous level.
    ///
    /// [UEFI Spec Documentation: 7.1.8. EFI_BOOT_SERVICES.RaiseTPL()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-raisetpl)
    fn raise_tpl(&self, tpl: Tpl) -> Tpl;

    /// Restores a task’s priority level to its previous value.
    ///
    /// [UEFI Spec Documentation: 7.1.9. EFI_BOOT_SERVICES.RestoreTPL()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-restoretpl)
    fn restore_tpl(&self, tpl: Tpl);

    /// Allocates memory pages from the system.
    ///
    /// [UEFI Spec Documentation: 7.2.1. EFI_BOOT_SERVICES.AllocatePages()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-allocatepages)
    fn allocate_pages(
        &self,
        alloc_type: AllocType,
        memory_type: MemoryType,
        nb_pages: usize,
    ) -> Result<usize, efi::Status>;

    /// Frees memory pages.
    ///
    /// [UEFI Spec Documentation: 7.2.2. EFI_BOOT_SERVICES.FreePages()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-freepages)
    fn free_pages(&self, address: usize, nb_pages: usize) -> Result<(), efi::Status>;

    /// Returns the current memory map.
    ///
    /// [UEFI Spec Documentation: 7.2.3. EFI_BOOT_SERVICES.GetMemoryMap()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-getmemorymap)
    fn get_memory_map<'a>(&'a self) -> Result<MemoryMap<'a, Self>, (efi::Status, usize)>;

    /// Allocates pool memory.
    ///
    /// [UEFI Spec Documentation: 7.2.4. EFI_BOOT_SERVICES.AllocatePool()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-allocatepool)
    fn allocate_pool(&self, pool_type: MemoryType, size: usize) -> Result<*mut u8, efi::Status>;

    /// Allocates pool memory casted as given type.
    fn allocate_pool_for_type<T: 'static>(&self, pool_type: MemoryType) -> Result<*mut T, efi::Status> {
        let ptr = self.allocate_pool(pool_type, mem::size_of::<T>())?;
        Ok(ptr as *mut T)
    }

    /// Returns pool memory to the system.
    ///
    /// [UEFI Spec Documentation: 7.2.5. EFI_BOOT_SERVICES.FreePool()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-freepool)
    fn free_pool(&self, buffer: *mut u8) -> Result<(), efi::Status>;

    /// Installs a protocol interface on a device handle.
    /// If the handle does not exist, it is created and added to the list of handles in the system.
    ///
    /// [UEFI Spec Documentation: 7.3.2. EFI_BOOT_SERVICES.InstallProtocolInterface()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-installprotocolinterface)
    fn install_protocol_interface<P: Protocol<Interface = I> + 'static, I: Any + 'static>(
        &self,
        handle: Option<efi::Handle>,
        protocol: &P,
        interface: &'static mut I,
    ) -> Result<efi::Handle, efi::Status> {
        let interface_ptr = match (interface as &dyn Any).downcast_ref::<()>() {
            Some(()) => ptr::null_mut(),
            None => interface as *mut _ as *mut c_void,
        };
        //SAFETY: The generic Protocol ensure that the interface is the right type for the specified protocol.
        unsafe { self.install_protocol_interface_unchecked(handle, protocol.protocol_guid(), interface_ptr) }
    }

    fn install_protocol_marker<P>(&self, handle: Option<efi::Handle>, protocol: &P) -> Result<efi::Handle, efi::Status>
    where
        P: Protocol<Interface = ()> + 'static,
    {
        unsafe { self.install_protocol_interface_unchecked(handle, protocol.protocol_guid(), ptr::null_mut()) }
    }

    fn install_protocol_interface_2<P, R, I>(
        &self,
        handle: Option<efi::Handle>,
        protocol: &P,
        interface: R,
    ) -> Result<(efi::Handle, PtrMetadata<'static, R>), efi::Status>
    where
        P: Protocol<Interface = I> + 'static,
        R: CMutRef<'static, Type = I> + 'static,
        I: 'static,
    {
        assert_ne!(
            TypeId::of::<()>(),
            TypeId::of::<I>(),
            "Marker interface are not supported by install_protocol_interface, use install_protocol_marker instead."
        );

        let key = interface.metadata();
        let handle = unsafe {
            self.install_protocol_interface_unchecked(
                handle,
                protocol.protocol_guid(),
                interface.into_mut_ptr() as *mut c_void,
            )?
        };
        Ok((handle, key))
    }

    /// Prefer normal [`BootServices::install_protocol_interface`] when possible.
    ///
    /// # Safety
    ///
    /// When calling this method, you have to make sure that if *interface* pointer is non-null, it is adhereing to
    /// the structure associated with the protocol.
    unsafe fn install_protocol_interface_unchecked(
        &self,
        handle: Option<efi::Handle>,
        protocol: &'static efi::Guid,
        interface: *mut c_void,
    ) -> Result<efi::Handle, efi::Status>;

    /// Removes a protocol interface from a device handle.
    ///
    /// [UEFI Spec Documentation: 7.3.3. EFI_BOOT_SERVICES.UninstallProtocolInterface()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-uninstallprotocolinterface)
    fn uninstall_protocol_interface<P: Protocol<Interface = I> + 'static, I: Any + 'static>(
        &self,
        handle: efi::Handle,
        protocol: &P,
        interface: &'static mut I,
    ) -> Result<(), efi::Status> {
        let interface_ptr = match (interface as &dyn Any).downcast_ref::<()>() {
            Some(()) => ptr::null_mut(),
            None => interface as *mut _ as *mut c_void,
        };
        //SAFETY: The generic Protocol ensure that the interface is the right type for the specified protocol.
        unsafe { self.uninstall_protocol_interface_unchecked(handle, protocol.protocol_guid(), interface_ptr) }
    }

    fn uninstall_protocol_marker<P>(&self, handle: efi::Handle, protocol: &P) -> Result<(), efi::Status>
    where
        P: Protocol<Interface = ()> + 'static,
    {
        unsafe { self.uninstall_protocol_interface_unchecked(handle, protocol.protocol_guid(), ptr::null_mut()) }?;
        Ok(())
    }

    fn uninstall_protocol_interface_2<P, R, I>(
        &self,
        handle: efi::Handle,
        protocol: &P,
        key: PtrMetadata<'static, R>,
    ) -> Result<R, efi::Status>
    where
        P: Protocol<Interface = I> + 'static,
        R: CMutRef<'static, Type = I> + 'static,
        I: 'static,
    {
        unsafe {
            self.uninstall_protocol_interface_unchecked(handle, protocol.protocol_guid(), key.ptr_value as *mut c_void)
        }?;
        Ok(unsafe { key.into_original_ptr() })
    }

    /// Prefer normal [`BootServices::uninstall_protocol_interface`] when possible.
    ///
    /// # Safety
    ///
    unsafe fn uninstall_protocol_interface_unchecked(
        &self,
        handle: efi::Handle,
        protocol: &'static efi::Guid,
        interface: *mut c_void,
    ) -> Result<(), efi::Status>;

    /// Reinstalls a protocol interface on a device handle.
    ///
    /// [UEFI Spec Documentation: 7.3.4. EFI_BOOT_SERVICES.ReinstallProtocolInterface()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-reinstallprotocolinterface)
    fn reinstall_protocol_interface<P: Protocol<Interface = I> + 'static, I: 'static>(
        &self,
        handle: efi::Handle,
        protocol: &P,
        old_protocol_interface: &'static mut I,
        new_protocol_interface: &'static mut I,
    ) -> Result<(), efi::Status> {
        let old_protocol_interface_ptr;
        let new_protocol_interface_ptr;
        if TypeId::of::<I>() == TypeId::of::<()>() {
            old_protocol_interface_ptr = ptr::null_mut();
            new_protocol_interface_ptr = ptr::null_mut();
        } else {
            old_protocol_interface_ptr = old_protocol_interface as *mut _ as *mut c_void;
            new_protocol_interface_ptr = new_protocol_interface as *mut _ as *mut c_void;
        }
        //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
        unsafe {
            self.reinstall_protocol_interface_unchecked(
                handle,
                protocol.protocol_guid(),
                old_protocol_interface_ptr,
                new_protocol_interface_ptr,
            )
        }
    }

    fn reinstall_protocol_interface_2<P, O, N, I>(
        &self,
        handle: efi::Handle,
        protocol: &P,
        old_protocol_interface_key: PtrMetadata<'static, O>,
        new_protocol_interface: N,
    ) -> Result<(PtrMetadata<'static, N>, O), efi::Status>
    where
        P: Protocol<Interface = I> + 'static,
        O: CMutRef<'static, Type = I> + 'static,
        N: CMutRef<'static, Type = I> + 'static,
        I: 'static,
    {
        assert_ne!(
            TypeId::of::<()>(),
            TypeId::of::<I>(),
            "Marker interface are not supported with reinstall_protocol_interface function."
        );
        let new_key = new_protocol_interface.metadata();
        unsafe {
            self.reinstall_protocol_interface_unchecked(
                handle,
                protocol.protocol_guid(),
                old_protocol_interface_key.ptr_value as *mut _,
                new_protocol_interface.into_mut_ptr() as *mut _,
            )?;
        }
        Ok((new_key, unsafe { old_protocol_interface_key.into_original_ptr() }))
    }

    /// Prefer normal [`BootServices::reinstall_protocol_interface`] when possible.
    ///
    /// # Safety
    /// When calling this method, you have to make sure that if *new_protocol_interface* pointer is non-null, it is adhereing to
    /// the structure associated with the protocol.
    unsafe fn reinstall_protocol_interface_unchecked(
        &self,
        handle: efi::Handle,
        protocol: &'static efi::Guid,
        old_protocol_interface: *mut c_void,
        new_protocol_interface: *mut c_void,
    ) -> Result<(), efi::Status>;

    /// Creates an event that is to be signaled whenever an interface is installed for a specified protocol.
    ///
    /// [UEFI Spec Documentation: 7.3.5. EFI_BOOT_SERVICES.RegisterProtocolNotify()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-registerprotocolnotify)
    fn register_protocol_notify(
        &self,
        protocol: &'static efi::Guid,
        event: efi::Event,
    ) -> Result<Registration, efi::Status>;

    /// Returns an array of handles that support a specified protocol.
    ///
    /// [UEFI Spec Documentation: 7.3.6. EFI_BOOT_SERVICES.LocateHandle()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-locatehandle)
    fn locate_handle<'a>(
        &'a self,
        search_type: HandleSearchType,
    ) -> Result<BootServicesBox<'a, [efi::Handle], Self>, efi::Status>;

    /// Queries a handle to determine if it supports a specified protocol.
    ///
    /// [UEFI Spec Documentation: 7.3.7. EFI_BOOT_SERVICES.HandleProtocol()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-handleprotocol)
    fn handle_protocol<P: Protocol<Interface = I> + 'static, I: 'static>(
        &self,
        handle: efi::Handle,
        protocol: &P,
    ) -> Result<&'static mut I, efi::Status> {
        //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
        unsafe {
            self.handle_protocol_unchecked(handle, protocol.protocol_guid()).map(|i| (i as *mut I).as_mut().unwrap())
        }
    }

    /// Prefer normal [`BootServices::handle_protocol`] when possible.
    ///
    /// # Safety
    ///
    unsafe fn handle_protocol_unchecked(
        &self,
        handle: efi::Handle,
        protocol: &efi::Guid,
    ) -> Result<*mut c_void, efi::Status>;

    /// Locates the handle to a device on the device path that supports the specified protocol.
    ///
    /// # Safety
    ///
    /// [UEFI Spec Documentation: 7.3.8. EFI_BOOT_SERVICES.LocateDevicePath()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-locatedevicepath)
    unsafe fn locate_device_path(
        &self,
        protocol: &efi::Guid,
        device_path: *mut *mut efi::protocols::device_path::Protocol,
    ) -> Result<efi::Handle, efi::Status>;

    /// Queries a handle to determine if it supports a specified protocol.
    /// If the protocol is supported by the handle, it opens the protocol on behalf of the calling agent.
    ///
    /// [UEFI Spec Documentation: 7.3.9. EFI_BOOT_SERVICES.OpenProtocol()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-openprotocol)
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
            self.open_protocol_unchecked(handle, protocol, agent_handle, controller_handle, attribute)
                .map(|i| (i as *mut I).as_mut())
        }
    }

    /// Prefer normal [`BootServices::open_protocol`] when possible.
    ///
    /// # Safety
    ///
    /// When calling this method, you have to make sure that if *agent_handle* pointer is non-null.
    unsafe fn open_protocol_unchecked(
        &self,
        handle: efi::Handle,
        protocol: &efi::Guid,
        agent_handle: efi::Handle,
        controller_handle: efi::Handle,
        attribute: u32,
    ) -> Result<*mut c_void, efi::Status>;

    /// Closes a protocol on a handle that was previously opened.
    ///
    /// [UEFI Spec Documentation: 7.3.10. EFI_BOOT_SERVICES.CloseProtocol()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-closeprotocol)
    fn close_protocol(
        &self,
        handle: efi::Handle,
        protocol: &efi::Guid,
        agent_handle: efi::Handle,
        controller_handle: efi::Handle,
    ) -> Result<(), efi::Status>;

    /// Retrieves the list of agents that currently have a protocol interface opened.
    ///
    /// [UEFI Spec Documentation: 7.3.11. EFI_BOOT_SERVICES.OpenProtocolInformation()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-openprotocolinformation)
    fn open_protocol_information<'a>(
        &'a self,
        handle: efi::Handle,
        protocol: &efi::Guid,
    ) -> Result<BootServicesBox<'a, [efi::OpenProtocolInformationEntry], Self>, efi::Status>;

    /// Connects one or more drivers to a controller.
    ///
    /// # Safety
    ///
    /// When calling this method, you have to make sure that *driver_image_handle*'s last entry is null per UEFI specification.
    ///
    /// [UEFI Spec Documentation: 7.3.12. EFI_BOOT_SERVICES.ConnectController()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-connectcontroller)
    unsafe fn connect_controller(
        &self,
        controller_handle: efi::Handle,
        driver_image_handles: Vec<efi::Handle>,
        remaining_device_path: *mut efi::protocols::device_path::Protocol,
        recursive: bool,
    ) -> Result<(), efi::Status>;

    /// Disconnects one or more drivers from a controller.
    ///
    /// [UEFI Spec Documentation: 7.3.13. EFI_BOOT_SERVICES.DisconnectController()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-disconnectcontroller)
    fn disconnect_controller(
        &self,
        controller_handle: efi::Handle,
        driver_image_handle: Option<efi::Handle>,
        child_handle: Option<efi::Handle>,
    ) -> Result<(), efi::Status>;

    /// Retrieves the list of protocol interface GUIDs that are installed on a handle in a buffer allocated from pool.
    ///
    /// [UEFI Spec Documentation: 7.3.14. EFI_BOOT_SERVICES.ProtocolsPerHandle()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-protocolsperhandle)
    fn protocols_per_handle<'a>(
        &'a self,
        handle: efi::Handle,
    ) -> Result<BootServicesBox<'a, [&'static efi::Guid], Self>, efi::Status>;

    /// Returns an array of handles that support the requested protocol in a buffer allocated from pool.
    ///
    /// [UEFI Spec Documentation: 7.3.15. EFI_BOOT_SERVICES.LocateHandleBuffer()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-locatehandlebuffer)
    fn locate_handle_buffer<'a>(
        &'a self,
        search_type: HandleSearchType,
    ) -> Result<BootServicesBox<'a, [efi::Handle], Self>, efi::Status>;

    /// Returns the first protocol instance that matches the given protocol.
    ///
    /// [UEFI Spec Documentation: 7.3.16. EFI_BOOT_SERVICES.LocateProtocol()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-locateprotocol)
    ///
    /// # Safety
    ///
    /// Make sure to not create multiple mutable reference when using this api.
    ///
    unsafe fn locate_protocol<P, I>(
        &self,
        protocol: &P,
        registration: Option<Registration>,
    ) -> Result<Option<&'static mut I>, efi::Status>
    where
        P: Protocol<Interface = I> + 'static,
        I: Any + 'static,
    {
        //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
        let interface_ptr = unsafe {
            self.locate_protocol_unchecked(
                protocol.protocol_guid(),
                registration.map_or(ptr::null_mut(), |r| r.as_ptr()),
            )? as *mut I
        };
        Ok(interface_ptr.as_mut())
    }

    unsafe fn locate_protocol_marker<P>(
        &self,
        protocol: &P,
        registration: Option<Registration>,
    ) -> Result<(), efi::Status>
    where
        P: Protocol<Interface = ()> + 'static,
    {
        //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
        let interface_ptr = unsafe {
            self.locate_protocol_unchecked(
                protocol.protocol_guid(),
                registration.map_or(ptr::null_mut(), |r| r.as_ptr()),
            )?
        };
        assert_eq!(ptr::null_mut(), interface_ptr);
        Ok(())
    }

    unsafe fn locate_protocol_2<P, I>(
        &self,
        protocol: &P,
        registration: Option<Registration>,
    ) -> Result<&'static mut I, efi::Status>
    where
        P: Protocol<Interface = I> + 'static,
        I: Any + 'static,
    {
        assert_ne!(
            TypeId::of::<()>(),
            TypeId::of::<I>(),
            "Marker interface are not supported by locate_protocol, use locate_protocol_marker instead."
        );

        //SAFETY: The generic Protocol ensure that the interfaces is the right type for the specified protocol.
        let interface_ptr = unsafe {
            self.locate_protocol_unchecked(
                protocol.protocol_guid(),
                registration.map_or(ptr::null_mut(), |r| r.as_ptr()),
            )? as *mut I
        };
        Ok(interface_ptr.as_mut().unwrap())
    }

    /// Prefer normal [`BootServices::locate_protocol`] when possible.
    ///
    /// # Safety
    ///
    unsafe fn locate_protocol_unchecked(
        &self,
        protocol: &'static efi::Guid,
        registration: *mut c_void,
    ) -> Result<*mut c_void, efi::Status>;

    /// Load an EFI image from a memory buffer.
    ///
    /// This uses [`Self::load_image`] behind the scene. This function assume that the request is not originating from the boot manager.
    ///
    fn load_image_from_source(
        &self,
        parent_image_handle: efi::Handle,
        device_path: *mut efi::protocols::device_path::Protocol,
        source_buffer: &[u8],
    ) -> Result<efi::Handle, efi::Status> {
        self.load_image(false, parent_image_handle, device_path, Some(source_buffer))
    }

    /// Load an EFI image from a file.
    ///
    /// This uses [`Self::load_image`] behind the scene. This function assume that the request is not originating from the boot manager.
    ///
    fn load_image_from_file(
        &self,
        parent_image_handle: efi::Handle,
        file_device_path: NonNull<efi::protocols::device_path::Protocol>,
    ) -> Result<efi::Handle, efi::Status> {
        self.load_image(false, parent_image_handle, file_device_path.as_ptr(), None)
    }

    /// Loads an EFI image into memory.
    ///
    /// [UEFI Spec Documentation: 7.4.1. EFI_BOOT_SERVICES.LoadImage()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-loadimage)
    ///
    fn load_image<'a>(
        &self,
        boot_policy: bool,
        parent_image_handle: efi::Handle,
        device_path: *mut efi::protocols::device_path::Protocol,
        source_buffer: Option<&'a [u8]>,
    ) -> Result<efi::Handle, efi::Status>;

    /// Transfers control to a loaded image’s entry point.
    ///
    /// [UEFI Spec Documentation: 7.4.2. EFI_BOOT_SERVICES.StartImage()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-startimage)
    ///
    fn start_image<'a>(
        &'a self,
        image_handle: efi::Handle,
    ) -> Result<(), (efi::Status, Option<BootServicesBox<'a, [u8], Self>>)>;

    /// Unloads an image.
    ///
    /// [UEFI Spec Documentation: 7.4.3. EFI_BOOT_SERVICES.UnloadImage()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-unloadimage)
    ///
    fn unload_image(&self, image_handle: efi::Handle) -> Result<(), efi::Status>;

    /// Terminates a loaded EFI image and returns control to boot services.
    ///
    /// [UEFI Spec Documentation: 7.4.5. EFI_BOOT_SERVICES.Exit()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-exit)
    ///
    fn exit<'a>(
        &'a self,
        image_handle: efi::Handle,
        exit_status: efi::Status,
        exit_data: Option<BootServicesBox<'a, [u8], Self>>,
    ) -> Result<(), efi::Status>;

    /// Terminates all boot services.
    ///
    /// [UEFI Spec Documentation: EFI_BOOT_SERVICES.ExitBootServices()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-exitbootservices)
    ///
    fn exit_boot_services(&self, image_handle: efi::Handle, map_key: usize) -> Result<(), efi::Status>;

    /// Sets the system’s watchdog timer.
    ///
    /// Note:  
    /// We deliberately choose to ignore the watchdog code and data parameters because we are not using them.
    /// Feel free to add those if needed.
    ///
    /// [UEFI Spec Documentation: 7.5.1. EFI_BOOT_SERVICES.SetWatchdogTimer()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-setwatchdogtimer)
    fn set_watchdog_timer(&self, timeout: usize) -> Result<(), efi::Status>;

    /// Induces a fine-grained stall
    ///
    /// [UEFI Spec Documentation: 7.5.2. EFI_BOOT_SERVICES.Stall()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-stall)
    fn stall(&self, microseconds: usize) -> Result<(), efi::Status>;

    /// Copies the contents of one buffer to another buffer.
    ///
    /// [UEFI Spec Documentation: 7.5.3. EFI_BOOT_SERVICES.CopyMem()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-copymem)
    fn copy_mem<T: 'static>(&self, dest: &mut T, src: &T) {
        unsafe { self.copy_mem_unchecked(dest as *mut T as _, src as *const T as _, mem::size_of::<T>()) }
    }

    /// Use of [`Self::copy_mem`] is preferable if the context allows it.
    ///
    /// # Safety
    ///
    /// dest and src must be valid pointer to a continuous chunk of memory of size length.
    unsafe fn copy_mem_unchecked(&self, dest: *mut c_void, src: *const c_void, length: usize);

    /// Fills a buffer with a specified value.
    ///
    /// [UEFI Spec Documentation: 7.5.4. EFI_BOOT_SERVICES.SetMem()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-setmem)
    fn set_mem(&self, buffer: &mut [u8], value: u8);

    /// Returns a monotonically increasing count for the platform.
    ///
    /// [UEFI Spec Documentation: 7.5.5. EFI_BOOT_SERVICES.GetNextMonotonicCount()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-getnextmonotoniccount)
    fn get_next_monotonic_count(&self) -> Result<u64, efi::Status>;

    /// Adds, updates, or removes a configuration table entry from the EFI System Table.
    ///
    /// [UEFI Spec Documentation: 7.5.6. EFI_BOOT_SERVICES.InstallConfigurationTable()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-installconfigurationtable)
    fn install_configuration_table<T: StaticPtrMut + 'static>(
        &self,
        guid: &efi::Guid,
        table: T,
    ) -> Result<(), efi::Status> {
        unsafe { self.install_configuration_table_unchecked(guid, table.into_raw_mut() as *mut c_void) }
    }

    /// Use [`BootServices::install_configuration_table`] when possible.
    ///
    /// # Safety
    ///
    /// The table pointer must be the right type associated with the guid.
    unsafe fn install_configuration_table_unchecked(
        &self,
        guid: &efi::Guid,
        table: *mut c_void,
    ) -> Result<(), efi::Status>;

    /// Computes and returns a 32-bit CRC for a data buffer.
    ///
    /// [UEFI Spec Documentation: 7.5.7. EFI_BOOT_SERVICES.CalculateCrc32()](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html#efi-boot-services-calculatecrc32)
    fn calculate_crc_32<T: 'static>(&self, data: &T) -> Result<u32, efi::Status> {
        unsafe { self.calculate_crc_32_unchecked(data as *const T as _, mem::size_of::<T>()) }
    }

    unsafe fn calculate_crc_32_unchecked(&self, data: *const c_void, data_size: usize) -> Result<u32, efi::Status>;
}

macro_rules! efi_boot_services_fn {
    ($efi_boot_services:expr, $fn_name:ident) => {{
        match $efi_boot_services.$fn_name {
            f if f as usize == 0 => panic!("Boot services function {} is not initialized.", stringify!($fn_name)),
            f => f,
        }
    }};
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
        let status = efi_boot_services_fn!(self.efi_boot_services(), create_event)(
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
        let status = efi_boot_services_fn!(self.efi_boot_services(), create_event_ex)(
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
        match efi_boot_services_fn!(self.efi_boot_services(), close_event)(event) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn signal_event(&self, event: efi::Event) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), signal_event)(event) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn wait_for_event(&self, events: &mut [efi::Event]) -> Result<usize, efi::Status> {
        let mut index = MaybeUninit::zeroed();
        let status = efi_boot_services_fn!(self.efi_boot_services(), wait_for_event)(
            events.len(),
            events.as_mut_ptr(),
            index.as_mut_ptr(),
        );
        if status.is_error() {
            Err(status)
        } else {
            Ok(unsafe { index.assume_init() })
        }
    }

    fn check_event(&self, event: efi::Event) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), check_event)(event) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn set_timer(&self, event: efi::Event, timer_type: EventTimerType, trigger_time: u64) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), set_timer)(event, timer_type.into(), trigger_time) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn raise_tpl(&self, new_tpl: Tpl) -> Tpl {
        efi_boot_services_fn!(self.efi_boot_services(), raise_tpl)(new_tpl.into()).into()
    }

    fn restore_tpl(&self, old_tpl: Tpl) {
        efi_boot_services_fn!(self.efi_boot_services(), restore_tpl)(old_tpl.into())
    }

    fn allocate_pages(
        &self,
        alloc_type: AllocType,
        memory_type: MemoryType,
        nb_pages: usize,
    ) -> Result<usize, efi::Status> {
        let mut memory_address = match alloc_type {
            AllocType::Address(address) => address,
            AllocType::MaxAddress(address) => address,
            _ => 0,
        };
        match efi_boot_services_fn!(self.efi_boot_services(), allocate_pages)(
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
        match efi_boot_services_fn!(self.efi_boot_services(), free_pages)(address as u64, nb_pages) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn get_memory_map<'a>(&'a self) -> Result<MemoryMap<'a, Self>, (efi::Status, usize)> {
        let get_memory_map = efi_boot_services_fn!(self.efi_boot_services(), get_memory_map);

        let mut memory_map_size = 0;
        let mut map_key = 0;
        let mut descriptor_size = 0;
        let mut descriptor_version = 0;

        match get_memory_map(
            ptr::addr_of_mut!(memory_map_size),
            ptr::null_mut(),
            ptr::addr_of_mut!(map_key),
            ptr::addr_of_mut!(descriptor_size),
            ptr::addr_of_mut!(descriptor_version),
        ) {
            s if s == efi::Status::BUFFER_TOO_SMALL => memory_map_size += 0x400, // add more space in case allocation makes the memory map bigger.
            _ => (),
        };

        let buffer = self.allocate_pool(MemoryType::BOOT_SERVICES_DATA, memory_map_size).map_err(|s| (s, 0))?;

        match get_memory_map(
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
            descriptors: unsafe { BootServicesBox::from_raw_parts_mut(buffer as *mut _, descriptor_size, self) },
            map_key,
            descriptor_version,
        })
    }

    fn allocate_pool(&self, memory_type: MemoryType, size: usize) -> Result<*mut u8, efi::Status> {
        let mut buffer = ptr::null_mut();
        match efi_boot_services_fn!(self.efi_boot_services(), allocate_pool)(
            memory_type.into(),
            size,
            ptr::addr_of_mut!(buffer),
        ) {
            s if s.is_error() => return Err(s),
            _ => Ok(buffer as *mut u8),
        }
    }

    fn free_pool(&self, buffer: *mut u8) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), free_pool)(buffer as *mut c_void) {
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
        match efi_boot_services_fn!(self.efi_boot_services(), install_protocol_interface)(
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
        interface: *mut c_void,
    ) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), uninstall_protocol_interface)(
            handle,
            protocol as *const _ as *mut _,
            interface,
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    unsafe fn reinstall_protocol_interface_unchecked(
        &self,
        handle: efi::Handle,
        protocol: &'static efi::Guid,
        old_protocol_interface: *mut c_void,
        new_protocol_interface: *mut c_void,
    ) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), reinstall_protocol_interface)(
            handle,
            protocol as *const _ as *mut _,
            old_protocol_interface,
            new_protocol_interface,
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn register_protocol_notify(&self, protocol: &efi::Guid, event: efi::Event) -> Result<Registration, efi::Status> {
        let mut registration = MaybeUninit::uninit();
        match efi_boot_services_fn!(self.efi_boot_services(), register_protocol_notify)(
            protocol as *const _ as *mut _,
            event,
            registration.as_mut_ptr() as *mut _,
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(unsafe { registration.assume_init() }),
        }
    }

    fn locate_handle(
        &self,
        search_type: HandleSearchType,
    ) -> Result<BootServicesBox<[efi::Handle], Self>, efi::Status> {
        let locate_handle = efi_boot_services_fn!(self.efi_boot_services(), locate_handle);

        let protocol = match search_type {
            HandleSearchType::ByProtocol(p) => p as *const _ as *mut _,
            _ => ptr::null_mut(),
        };
        let search_key = match search_type {
            HandleSearchType::ByRegisterNotify(r) => r.as_ptr(),
            _ => ptr::null_mut(),
        };

        // Use to get the buffer_size
        let mut buffer_size = 0;
        locate_handle(search_type.into(), protocol, search_key, ptr::addr_of_mut!(buffer_size), ptr::null_mut());

        let buffer = self.allocate_pool(MemoryType::BOOT_SERVICES_DATA, buffer_size)?;

        match locate_handle(
            search_type.into(),
            protocol,
            search_key,
            ptr::addr_of_mut!(buffer_size),
            buffer as *mut efi::Handle,
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(unsafe {
                BootServicesBox::from_raw_parts_mut(
                    buffer as *mut _,
                    buffer_size / mem::size_of::<efi::Handle>(),
                    &self,
                )
            }),
        }
    }

    unsafe fn handle_protocol_unchecked(
        &self,
        handle: efi::Handle,
        protocol: &efi::Guid,
    ) -> Result<*mut c_void, efi::Status> {
        let mut interface = ptr::null_mut();
        match efi_boot_services_fn!(self.efi_boot_services(), handle_protocol)(
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
        match efi_boot_services_fn!(self.efi_boot_services(), locate_device_path)(
            protocol as *const _ as *mut _,
            device_path,
            ptr::addr_of_mut!(device),
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(device),
        }
    }

    unsafe fn open_protocol_unchecked(
        &self,
        handle: efi::Handle,
        protocol: &efi::Guid,
        agent_handle: efi::Handle,
        controller_handle: efi::Handle,
        attribute: u32,
    ) -> Result<*mut c_void, efi::Status> {
        let mut interface = ptr::null_mut();
        match efi_boot_services_fn!(self.efi_boot_services(), open_protocol)(
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
        match efi_boot_services_fn!(self.efi_boot_services(), close_protocol)(
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
        match efi_boot_services_fn!(self.efi_boot_services(), open_protocol_information)(
            handle,
            protocol as *const _ as *mut _,
            ptr::addr_of_mut!(entry_buffer),
            ptr::addr_of_mut!(entry_count),
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(unsafe { BootServicesBox::from_raw_parts_mut(entry_buffer, entry_count, self) }),
        }
    }

    unsafe fn connect_controller(
        &self,
        controller_handle: efi::Handle,
        mut driver_image_handles: Vec<efi::Handle>,
        remaining_device_path: *mut efi::protocols::device_path::Protocol,
        recursive: bool,
    ) -> Result<(), efi::Status> {
        let driver_image_handles = if driver_image_handles.is_empty() {
            ptr::null_mut()
        } else {
            driver_image_handles.push(ptr::null_mut());
            driver_image_handles.as_mut_ptr()
        };
        match efi_boot_services_fn!(self.efi_boot_services(), connect_controller)(
            controller_handle,
            driver_image_handles,
            remaining_device_path,
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
        match efi_boot_services_fn!(self.efi_boot_services(), disconnect_controller)(
            controller_handle,
            driver_image_handle.unwrap_or(ptr::null_mut()),
            child_handle.unwrap_or(ptr::null_mut()),
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn protocols_per_handle(
        &self,
        handle: efi::Handle,
    ) -> Result<BootServicesBox<[&'static efi::Guid], Self>, efi::Status> {
        let mut protocol_buffer = ptr::null_mut();
        let mut protocol_buffer_count = 0;
        match efi_boot_services_fn!(self.efi_boot_services(), protocols_per_handle)(
            handle,
            ptr::addr_of_mut!(protocol_buffer),
            ptr::addr_of_mut!(protocol_buffer_count),
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(unsafe {
                BootServicesBox::<[_], _>::from_raw_parts_mut(protocol_buffer as *mut _, protocol_buffer_count, self)
            }),
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
            HandleSearchType::ByRegisterNotify(r) => r.as_ptr(),
            _ => ptr::null_mut(),
        };
        match efi_boot_services_fn!(self.efi_boot_services(), locate_handle_buffer)(
            search_type.into(),
            protocol,
            search_key,
            ptr::addr_of_mut!(buffer_count),
            ptr::addr_of_mut!(buffer),
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(unsafe {
                BootServicesBox::<[_], _>::from_raw_parts_mut(buffer as *mut efi::Handle, buffer_count, self)
            }),
        }
    }

    unsafe fn locate_protocol_unchecked(
        &self,
        protocol: &'static efi::Guid,
        registration: *mut c_void,
    ) -> Result<*mut c_void, efi::Status> {
        let mut interface = ptr::null_mut();
        match efi_boot_services_fn!(self.efi_boot_services(), locate_protocol)(
            protocol as *const _ as *mut _,
            registration,
            ptr::addr_of_mut!(interface),
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(interface),
        }
    }

    fn load_image(
        &self,
        boot_policy: bool,
        parent_image_handle: efi::Handle,
        device_path: *mut efi::protocols::device_path::Protocol,
        source_buffer: Option<&[u8]>,
    ) -> Result<efi::Handle, efi::Status> {
        let source_buffer_ptr =
            source_buffer.map_or(ptr::null_mut(), |buffer| buffer.as_ptr() as *const _ as *mut c_void);
        let source_buffer_size = source_buffer.map_or(0, |buffer| buffer.len());
        let mut image_handle = MaybeUninit::uninit();
        match efi_boot_services_fn!(self.efi_boot_services(), load_image)(
            boot_policy.into(),
            parent_image_handle,
            device_path,
            source_buffer_ptr,
            source_buffer_size,
            image_handle.as_mut_ptr(),
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(unsafe { image_handle.assume_init() }),
        }
    }

    fn start_image<'a>(
        &'a self,
        image_handle: efi::Handle,
    ) -> Result<(), (efi::Status, Option<BootServicesBox<'a, [u8], Self>>)> {
        let mut exit_data_size = MaybeUninit::uninit();
        let mut exit_data = MaybeUninit::uninit();
        match efi_boot_services_fn!(self.efi_boot_services(), start_image)(
            image_handle,
            exit_data_size.as_mut_ptr(),
            exit_data.as_mut_ptr(),
        ) {
            s if s.is_error() => {
                let data = (!exit_data.as_ptr().is_null()).then(|| unsafe {
                    BootServicesBox::from_raw_parts_mut(
                        exit_data.as_mut_ptr() as *mut u8,
                        exit_data_size.assume_init(),
                        self,
                    )
                });
                Err((s, data))
            }
            _ => Ok(()),
        }
    }

    fn unload_image(&self, image_handle: efi::Handle) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), unload_image)(image_handle) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn exit<'a>(
        &'a self,
        image_handle: efi::Handle,
        exit_status: efi::Status,
        exit_data: Option<BootServicesBox<'a, [u8], Self>>,
    ) -> Result<(), efi::Status> {
        let exit_data_ptr = exit_data.as_ref().map_or(ptr::null_mut(), |data| data.as_ptr() as *mut u16);
        let exit_data_size = exit_data.as_ref().map_or(0, |data| data.len());
        match efi_boot_services_fn!(self.efi_boot_services(), exit)(
            image_handle,
            exit_status,
            exit_data_size,
            exit_data_ptr,
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn exit_boot_services(&self, image_handle: efi::Handle, map_key: usize) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), exit_boot_services)(image_handle, map_key) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn set_watchdog_timer(&self, timeout: usize) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), set_watchdog_timer)(timeout, 0, 0, ptr::null_mut()) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    fn stall(&self, microseconds: usize) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), stall)(microseconds) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    unsafe fn copy_mem_unchecked(&self, dest: *mut c_void, src: *const c_void, length: usize) {
        efi_boot_services_fn!(self.efi_boot_services(), copy_mem)(dest, src as *mut _, length);
    }

    fn set_mem(&self, buffer: &mut [u8], value: u8) {
        efi_boot_services_fn!(self.efi_boot_services(), set_mem)(
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len(),
            value,
        );
    }

    fn get_next_monotonic_count(&self) -> Result<u64, efi::Status> {
        let mut count = MaybeUninit::uninit();
        match efi_boot_services_fn!(self.efi_boot_services(), get_next_monotonic_count)(count.as_mut_ptr()) {
            s if s.is_error() => Err(s),
            _ => Ok(unsafe { count.assume_init() }),
        }
    }

    unsafe fn install_configuration_table_unchecked(
        &self,
        guid: &efi::Guid,
        table: *mut c_void,
    ) -> Result<(), efi::Status> {
        match efi_boot_services_fn!(self.efi_boot_services(), install_configuration_table)(
            guid as *const _ as *mut _,
            table,
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(()),
        }
    }

    unsafe fn calculate_crc_32_unchecked(&self, data: *const c_void, data_size: usize) -> Result<u32, efi::Status> {
        let mut crc32 = MaybeUninit::uninit();
        match efi_boot_services_fn!(self.efi_boot_services(), calculate_crc32)(
            data as *mut _,
            data_size,
            crc32.as_mut_ptr(),
        ) {
            s if s.is_error() => Err(s),
            _ => Ok(unsafe { crc32.assume_init() }),
        }
    }
}

#[cfg(test)]
mod test {
    use efi::{protocols::device_path, Boolean, Char16, OpenProtocolInformationEntry};
    use ffi_helper::CPtr;

    use super::*;
    use core::{mem::MaybeUninit, ops::Deref, slice, sync::atomic::AtomicUsize, u32, u64};
    use std::os::raw::c_void;

    macro_rules! boot_services {
        ($($efi_services:ident = $efi_service_fn:ident),*) => {{
            static BOOT_SERVICE: StandardBootServices = StandardBootServices::new_uninit();
            let efi_boot_services = unsafe {
                #[allow(unused_mut)]
                let mut bs = MaybeUninit::<efi::BootServices>::zeroed();
                $(
                bs.assume_init_mut().$efi_services = $efi_service_fn;
                )*
                bs.assume_init()
            };
            BOOT_SERVICE.initialize(&efi_boot_services);
            &BOOT_SERVICE
        }};
    }

    static TEST_PROTOCOL_GUID: efi::Guid =
        efi::Guid::from_bytes(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);

    struct TestProtocol;
    unsafe impl Protocol for TestProtocol {
        type Interface = u32;

        fn protocol_guid(&self) -> &'static efi::Guid {
            &TEST_PROTOCOL_GUID
        }
    }

    impl Deref for TestProtocol {
        type Target = efi::Guid;

        fn deref(&self) -> &Self::Target {
            self.protocol_guid()
        }
    }

    struct TestProtocolMarker;

    unsafe impl Protocol for TestProtocolMarker {
        type Interface = ();

        fn protocol_guid(&self) -> &'static efi::Guid {
            &TEST_PROTOCOL_GUID
        }
    }

    impl Deref for TestProtocolMarker {
        type Target = efi::Guid;

        fn deref(&self) -> &Self::Target {
            self.protocol_guid()
        }
    }

    extern "efiapi" fn efi_allocate_pool_use_box(
        _mem_type: efi::MemoryType,
        size: usize,
        buffer: *mut *mut c_void,
    ) -> efi::Status {
        // Use u64 for ptr alignment.
        let allocation = vec![0 as u64; size.div_ceil(mem::size_of::<u64>())].into_boxed_slice();
        unsafe {
            *buffer = Box::into_raw(allocation) as *mut c_void;
        }
        efi::Status::SUCCESS
    }

    extern "efiapi" fn efi_free_pool_use_box(buffer: *mut c_void) -> efi::Status {
        if buffer.is_null() {
            return efi::Status::INVALID_PARAMETER;
        }

        unsafe {
            let _ = Box::from_raw(buffer as *mut u8);
        }

        efi::Status::SUCCESS
    }

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
    #[should_panic = "Boot services function create_event is not initialized."]
    fn test_create_event_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.create_event(EventType::RUNTIME, Tpl::APPLICATION, None, &());
    }

    #[test]
    fn test_create_event() {
        let boot_services = boot_services!(create_event = efi_create_event);

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
        let status = boot_services.create_event(
            EventType::RUNTIME | EventType::NOTIFY_SIGNAL,
            Tpl::APPLICATION,
            Some(notify_callback),
            ctx,
        );

        assert!(matches!(status, Ok(_)));
    }

    #[test]
    fn test_create_event_no_notify() {
        let boot_services = boot_services!(create_event = efi_create_event);

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

        let status =
            boot_services.create_event(EventType::RUNTIME | EventType::NOTIFY_SIGNAL, Tpl::APPLICATION, None, &());

        assert!(matches!(status, Ok(_)));
    }

    #[test]
    #[should_panic = "Boot services function create_event_ex is not initialized."]
    fn test_create_event_ex_not_init() {
        static GUID: efi::Guid = efi::Guid::from_fields(0, 0, 0, 0, 0, &[0; 6]);
        let boot_services = boot_services!();
        let _ = boot_services.create_event_ex(EventType::RUNTIME, Tpl::APPLICATION, None, &(), &GUID);
    }

    #[test]
    fn test_create_event_ex() {
        let boot_services = boot_services!(create_event_ex = efi_create_event_ex);

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
        let status = boot_services.create_event_ex(
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
        let boot_services = boot_services!(create_event_ex = efi_create_event_ex);

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
        let status = boot_services.create_event_ex(
            EventType::RUNTIME | EventType::NOTIFY_SIGNAL,
            Tpl::APPLICATION,
            None,
            &(),
            &GUID,
        );

        assert!(matches!(status, Ok(_)));
    }

    #[test]
    #[should_panic = "Boot services function close_event is not initialized."]
    fn test_close_event_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.close_event(ptr::null_mut());
    }

    #[test]
    fn test_close_event() {
        let boot_services = boot_services!(close_event = efi_close_event);

        extern "efiapi" fn efi_close_event(event: efi::Event) -> efi::Status {
            assert_eq!(1, event as usize);
            efi::Status::SUCCESS
        }

        let event = 1_usize as efi::Event;
        let status = boot_services.close_event(event);
        assert!(matches!(status, Ok(())));
    }

    #[test]
    #[should_panic = "Boot services function signal_event is not initialized."]
    fn test_signal_event_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.signal_event(ptr::null_mut());
    }

    #[test]
    fn test_signal_event() {
        let boot_services = boot_services!(signal_event = efi_signal_event);

        extern "efiapi" fn efi_signal_event(event: efi::Event) -> efi::Status {
            assert_eq!(1, event as usize);
            efi::Status::SUCCESS
        }

        let event = 1_usize as efi::Event;
        let status = boot_services.signal_event(event);
        assert!(matches!(status, Ok(())));
    }

    #[test]
    #[should_panic = "Boot services function wait_for_event is not initialized."]
    fn test_wait_for_event_not_init() {
        let boot_services = boot_services!();
        let mut events = vec![];
        let _ = boot_services.wait_for_event(&mut events);
    }

    #[test]
    fn test_wait_for_event() {
        let boot_services = boot_services!(wait_for_event = efi_wait_for_event);

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
        let status = boot_services.wait_for_event(&mut events);
        assert!(matches!(status, Ok(1)));
    }

    #[test]
    #[should_panic = "Boot services function check_event is not initialized."]
    fn test_check_event_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.check_event(ptr::null_mut());
    }

    #[test]
    fn test_check_event() {
        let boot_services = boot_services!(check_event = efi_check_event);

        extern "efiapi" fn efi_check_event(event: efi::Event) -> efi::Status {
            assert_eq!(1, event as usize);
            efi::Status::SUCCESS
        }

        let event = 1_usize as efi::Event;
        let status = boot_services.check_event(event);
        assert!(matches!(status, Ok(())));
    }

    #[test]
    #[should_panic = "Boot services function set_timer is not initialized."]
    fn test_set_timer_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.set_timer(ptr::null_mut(), EventTimerType::Relative, 0);
    }

    #[test]
    fn test_set_timer() {
        let boot_services = boot_services!(set_timer = efi_set_timer);

        extern "efiapi" fn efi_set_timer(event: efi::Event, r#type: efi::TimerDelay, trigger_time: u64) -> efi::Status {
            assert_eq!(1, event as usize);
            assert_eq!(efi::TIMER_PERIODIC, r#type);
            assert_eq!(200, trigger_time);
            efi::Status::SUCCESS
        }

        let event = 1_usize as efi::Event;
        let status = boot_services.set_timer(event, EventTimerType::Periodic, 200);
        assert!(matches!(status, Ok(())));
    }

    #[test]
    fn test_raise_tpl_guarded() {
        let boot_services = boot_services!(raise_tpl = efi_raise_tpl, restore_tpl = efi_restore_tpl);

        static CURRENT_TPL: AtomicUsize = AtomicUsize::new(efi::TPL_APPLICATION);

        extern "efiapi" fn efi_raise_tpl(tpl: efi::Tpl) -> efi::Tpl {
            assert_eq!(efi::TPL_NOTIFY, tpl);
            CURRENT_TPL.swap(tpl, Ordering::Relaxed)
        }

        extern "efiapi" fn efi_restore_tpl(tpl: efi::Tpl) {
            assert_eq!(efi::TPL_APPLICATION, tpl);
            CURRENT_TPL.swap(tpl, Ordering::Relaxed);
        }

        let guard = boot_services.raise_tpl_guarded(Tpl::NOTIFY);
        assert_eq!(Tpl::APPLICATION, guard.retore_tpl);
        assert_eq!(efi::TPL_NOTIFY, CURRENT_TPL.load(Ordering::Relaxed));
        drop(guard);
        assert_eq!(efi::TPL_APPLICATION, CURRENT_TPL.load(Ordering::Relaxed));
    }

    #[test]
    #[should_panic = "Boot services function raise_tpl is not initialized."]
    fn test_raise_tpl_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.raise_tpl(Tpl::CALLBACK);
    }

    #[test]
    fn test_raise_tpl() {
        let boot_services = boot_services!(raise_tpl = efi_raise_tpl);

        extern "efiapi" fn efi_raise_tpl(tpl: efi::Tpl) -> efi::Tpl {
            assert_eq!(efi::TPL_NOTIFY, tpl);
            efi::TPL_APPLICATION
        }

        let status = boot_services.raise_tpl(Tpl::NOTIFY);
        assert_eq!(Tpl::APPLICATION, status);
    }

    #[test]
    #[should_panic = "Boot services function restore_tpl is not initialized."]
    fn test_restore_tpl_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.restore_tpl(Tpl::APPLICATION);
    }

    #[test]
    fn test_restore_tpl() {
        let boot_services = boot_services!(restore_tpl = efi_restore_tpl);

        extern "efiapi" fn efi_restore_tpl(tpl: efi::Tpl) {
            assert_eq!(efi::TPL_APPLICATION, tpl);
        }

        boot_services.restore_tpl(Tpl::APPLICATION);
    }

    #[test]
    #[should_panic = "Boot services function allocate_pages is not initialized."]
    fn test_allocate_pages_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.allocate_pages(AllocType::AnyPage, MemoryType::ACPI_MEMORY_NVS, 0);
    }

    #[test]
    fn test_allocate_pages() {
        let boot_services = boot_services!(allocate_pages = efi_allocate_pages);

        extern "efiapi" fn efi_allocate_pages(
            alloc_type: u32,
            mem_type: u32,
            nb_pages: usize,
            memory: *mut u64,
        ) -> efi::Status {
            let expected_alloc_type: efi::AllocateType = AllocType::AnyPage.into();
            assert_eq!(expected_alloc_type, alloc_type);
            let expected_mem_type: efi::MemoryType = MemoryType::MEMORY_MAPPED_IO.into();
            assert_eq!(expected_mem_type, mem_type);
            assert_eq!(4, nb_pages);
            assert_ne!(ptr::null_mut(), memory);
            assert_eq!(0, unsafe { *memory });
            unsafe { ptr::write(memory, 17) }
            efi::Status::SUCCESS
        }

        let status = boot_services.allocate_pages(AllocType::AnyPage, MemoryType::MEMORY_MAPPED_IO, 4);

        assert!(matches!(status, Ok(17)));
    }

    #[test]
    fn test_allocate_pages_at_specific_address() {
        let boot_services = boot_services!(allocate_pages = efi_allocate_pages);

        extern "efiapi" fn efi_allocate_pages(
            alloc_type: u32,
            mem_type: u32,
            nb_pages: usize,
            memory: *mut u64,
        ) -> efi::Status {
            let expected_alloc_type: efi::AllocateType = AllocType::Address(17).into();
            assert_eq!(expected_alloc_type, alloc_type);
            let expected_mem_type: efi::MemoryType = MemoryType::MEMORY_MAPPED_IO.into();
            assert_eq!(expected_mem_type, mem_type);
            assert_eq!(4, nb_pages);
            assert_ne!(ptr::null_mut(), memory);
            assert_eq!(17, unsafe { *memory });
            efi::Status::SUCCESS
        }

        let status = boot_services.allocate_pages(AllocType::Address(17), MemoryType::MEMORY_MAPPED_IO, 4);
        assert!(matches!(status, Ok(17)));
    }

    #[test]
    #[should_panic = "Boot services function free_pages is not initialized."]
    fn test_free_pages_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.free_pages(0, 0);
    }

    #[test]
    fn test_free_pages() {
        let boot_services = boot_services!(free_pages = efi_free_pages);

        extern "efiapi" fn efi_free_pages(address: efi::PhysicalAddress, nb_pages: usize) -> efi::Status {
            assert_eq!(address, 0x100000);
            assert_eq!(nb_pages, 10);

            efi::Status::SUCCESS
        }

        let status = boot_services.free_pages(0x100000, 10);
        assert!(matches!(status, Ok(())));
    }

    #[test]
    #[should_panic = "Boot services function allocate_pool is not initialized."]
    fn test_allocate_pool_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.allocate_pool(MemoryType::RESERVED_MEMORY_TYPE, 0);
    }

    #[test]
    fn test_allocate_pool() {
        let boot_services = boot_services!(allocate_pool = efi_allocate_pool);

        extern "efiapi" fn efi_allocate_pool(
            mem_type: efi::MemoryType,
            size: usize,
            buffer: *mut *mut c_void,
        ) -> efi::Status {
            let expected_mem_type: efi::MemoryType = MemoryType::MEMORY_MAPPED_IO.into();
            assert_eq!(mem_type, expected_mem_type);
            assert_eq!(size, 10);
            unsafe { ptr::write(buffer, 0x55AA as *mut c_void) };
            efi::Status::SUCCESS
        }

        let status = boot_services.allocate_pool(MemoryType::MEMORY_MAPPED_IO, 10);
        assert_eq!(status, Ok(0x55AA as *mut u8));
    }

    #[test]
    fn test_free_pool() {
        let boot_services = boot_services!(free_pool = efi_free_pool);

        extern "efiapi" fn efi_free_pool(buffer: *mut c_void) -> efi::Status {
            if buffer.is_null() {
                return efi::Status::INVALID_PARAMETER;
            } else {
                assert_eq!(buffer, 0xffff0000 as *mut u8 as *mut c_void);
                return efi::Status::SUCCESS;
            }
        }

        // positive test
        let status = boot_services.free_pool(0xffff0000 as *mut u8);
        assert_eq!(status, Ok(()));

        // negative test
        let status = boot_services.free_pool(ptr::null_mut());
        assert_eq!(status, Err(efi::Status::INVALID_PARAMETER));
    }

    #[test]
    #[should_panic = "Boot services function install_protocol_interface is not initialized."]
    fn test_install_protocol_interface_not_init() {
        let boot_services = boot_services!();
        let _ = boot_services.install_protocol_interface_2(None, &TestProtocol, Box::new(0));
    }

    #[test]
    fn test_install_protocol_interface() {
        let boot_services = boot_services!(install_protocol_interface = efi_install_protocol_interface);

        extern "efiapi" fn efi_install_protocol_interface(
            handle: *mut efi::Handle,
            guid: *mut efi::Guid,
            interface_type: u32,
            interface: *mut c_void,
        ) -> efi::Status {
            assert_ne!(ptr::null_mut(), handle);
            assert_eq!(ptr::null_mut(), unsafe { ptr::read(handle) });
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(guid) });
            assert_eq!(efi::NATIVE_INTERFACE, interface_type);
            assert_eq!(42, unsafe { ptr::read(interface as *mut u32) });

            unsafe {
                ptr::write(handle, 17 as usize as _);
            }

            efi::Status::SUCCESS
        }

        let (handle, _) = boot_services.install_protocol_interface_2(None, &TestProtocol, Box::new(42)).unwrap();

        assert_eq!(17, handle as usize);
    }

    #[test]
    fn test_install_protocol_marker() {
        let boot_services = boot_services!(install_protocol_interface = efi_install_protocol_interface);

        extern "efiapi" fn efi_install_protocol_interface(
            handle: *mut efi::Handle,
            guid: *mut efi::Guid,
            interface_type: u32,
            interface: *mut c_void,
        ) -> efi::Status {
            assert_ne!(ptr::null_mut(), handle);
            assert_eq!(ptr::null_mut(), unsafe { ptr::read(handle) });
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(guid) });
            assert_eq!(efi::NATIVE_INTERFACE, interface_type);
            assert_eq!(ptr::null_mut(), interface);

            unsafe {
                ptr::write(handle, 17 as usize as _);
            }

            efi::Status::SUCCESS
        }

        let handle = boot_services.install_protocol_marker(None, &TestProtocolMarker).unwrap();

        assert_eq!(17, handle as usize);
    }

    #[test]
    #[should_panic = "Marker interface are not supported by install_protocol_interface, use install_protocol_marker instead."]
    fn test_install_protocol_marker_with_intall_protocol_interface() {
        let boot_services = boot_services!(install_protocol_interface = efi_install_protocol_interface);

        extern "efiapi" fn efi_install_protocol_interface(
            _handle: *mut efi::Handle,
            _guid: *mut efi::Guid,
            _interface_type: u32,
            _interface: *mut c_void,
        ) -> efi::Status {
            efi::Status::SUCCESS
        }

        _ = boot_services.install_protocol_interface_2(None, &TestProtocolMarker, Box::new(())).unwrap();
    }
    #[test]
    #[should_panic = "Boot services function uninstall_protocol_interface is not initialized."]
    fn test_uninstall_protocol_interface_not_init() {
        let boot_services = boot_services!();
        let b = Box::new(10);
        let key = CPtr::metadata(&b);
        _ = boot_services.uninstall_protocol_interface_2(ptr::null_mut(), &TestProtocol, key);
    }

    #[test]
    fn test_uninstall_protocol_interface() {
        let boot_services = boot_services!(uninstall_protocol_interface = efi_uninstall_protocol_interface);

        static ADDR: AtomicUsize = AtomicUsize::new(0);

        extern "efiapi" fn efi_uninstall_protocol_interface(
            handle: efi::Handle,
            protocol: *mut efi::Guid,
            interface: *mut c_void,
        ) -> efi::Status {
            assert_eq!(1, handle as usize);
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_eq!(ADDR.load(Ordering::Relaxed), interface as usize);
            efi::Status::SUCCESS
        }
        let b = Box::new(10);
        let key = b.metadata();
        ADDR.store(key.ptr_value, Ordering::Relaxed);
        let ptr = b.into_ptr();
        let interface = boot_services.uninstall_protocol_interface_2(1 as usize as _, &TestProtocol, key).unwrap();
        assert_eq!(ptr, interface.as_ptr());
    }

    #[test]
    fn test_uninstall_protocol_marker() {
        let boot_services = boot_services!(uninstall_protocol_interface = efi_uninstall_protocol_interface);

        extern "efiapi" fn efi_uninstall_protocol_interface(
            handle: efi::Handle,
            protocol: *mut efi::Guid,
            interface: *mut c_void,
        ) -> efi::Status {
            assert_eq!(1, handle as usize);
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_eq!(ptr::null_mut(), interface);
            efi::Status::SUCCESS
        }
        _ = boot_services.uninstall_protocol_marker(1 as usize as _, &TestProtocolMarker).unwrap();
    }

    #[test]
    #[should_panic = "Boot services function reinstall_protocol_interface is not initialized."]
    fn test_reinstall_protocol_interface_not_init() {
        let boot_services = boot_services!();
        let b = Box::new(10);
        let key = CPtr::metadata(&b);
        _ = boot_services.reinstall_protocol_interface_2(ptr::null_mut(), &TestProtocol, key, Box::new(10));
    }

    #[test]
    fn test_reinstall_protocol_interface() {
        let boot_services = boot_services!(reinstall_protocol_interface = efi_reinstall_protocol_interface);

        extern "efiapi" fn efi_reinstall_protocol_interface(
            handle: efi::Handle,
            protocol: *mut efi::Guid,
            old_interface: *mut c_void,
            new_interface: *mut c_void,
        ) -> efi::Status {
            assert_eq!(1, handle as usize);
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_ne!(ptr::null_mut(), old_interface);
            assert_ne!(ptr::null_mut(), new_interface);
            efi::Status::SUCCESS
        }

        let old_interface = Box::new(10);
        let old_key = CPtr::metadata(&old_interface);
        let old_ptr = old_interface.into_ptr();
        let new_interface = Box::new(10);
        let new_ptr = new_interface.as_ptr();
        let (new_key, old_interface) = boot_services
            .reinstall_protocol_interface_2(1 as usize as _, &TestProtocol, old_key, new_interface)
            .unwrap();
        assert_eq!(new_key.ptr_value, new_ptr as usize);
        assert_eq!(old_ptr, old_interface.into_ptr());
    }

    #[test]
    #[should_panic = "Boot services function register_protocol_notify is not initialized."]
    fn test_register_protocol_notify_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.register_protocol_notify(&TEST_PROTOCOL_GUID, ptr::null_mut());
    }

    #[test]
    fn test_register_protocol_notify() {
        let boot_services = boot_services!(register_protocol_notify = efi_register_protocol_notify);

        extern "efiapi" fn efi_register_protocol_notify(
            protocol: *mut efi::Guid,
            event: *mut c_void,
            registration: *mut *mut c_void,
        ) -> efi::Status {
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_eq!(1, event as usize);
            assert_ne!(ptr::null_mut(), registration);
            unsafe { ptr::write(registration, 10 as usize as _) };
            efi::Status::SUCCESS
        }

        let registration = boot_services.register_protocol_notify(&TEST_PROTOCOL_GUID, 1 as usize as _).unwrap();
        assert_eq!(10, registration.as_ptr() as usize);
    }

    #[test]
    #[should_panic = "Boot services function locate_handle is not initialized."]
    fn test_locate_handle_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.locate_handle(HandleSearchType::AllHandle);
    }

    #[test]
    fn test_locate_handle_all_handles() {
        let boot_services = boot_services!(
            locate_handle = efi_locate_handle,
            allocate_pool = efi_allocate_pool_use_box,
            free_pool = efi_free_pool_use_box
        );

        extern "efiapi" fn efi_locate_handle(
            search_type: efi::LocateSearchType,
            protocol: *mut efi::Guid,
            search_key: *mut c_void,
            buffer_size: *mut usize,
            buffer: *mut efi::Handle,
        ) -> efi::Status {
            assert_eq!(efi::ALL_HANDLES, search_type);
            assert_eq!(ptr::null_mut(), protocol);
            assert_eq!(ptr::null_mut(), search_key);
            assert_ne!(ptr::null_mut(), buffer_size);

            match buffer {
                buffer if buffer.is_null() => {
                    assert_eq!(0, unsafe { ptr::read(buffer_size) });
                    unsafe { ptr::write(buffer_size, 1 * mem::size_of::<usize>()) };
                }
                _ => {
                    unsafe { ptr::write(buffer, 10 as usize as _) };
                }
            }

            efi::Status::SUCCESS
        }

        let handles = boot_services.locate_handle(HandleSearchType::AllHandle).unwrap();
        assert_eq!(1, handles.len());
        assert_eq!(10, handles[0] as usize);
    }

    #[test]
    fn test_locate_handle_by_protocol() {
        let boot_services = boot_services!(
            locate_handle = efi_locate_handle,
            allocate_pool = efi_allocate_pool_use_box,
            free_pool = efi_free_pool_use_box
        );

        extern "efiapi" fn efi_locate_handle(
            search_type: efi::LocateSearchType,
            protocol: *mut efi::Guid,
            search_key: *mut c_void,
            buffer_size: *mut usize,
            buffer: *mut efi::Handle,
        ) -> efi::Status {
            assert_eq!(efi::BY_PROTOCOL, search_type);
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_eq!(ptr::null_mut(), search_key);
            assert_ne!(ptr::null_mut(), buffer_size);

            match buffer {
                buffer if buffer.is_null() => {
                    assert_eq!(0, unsafe { ptr::read(buffer_size) });
                    unsafe { ptr::write(buffer_size, 1 * mem::size_of::<usize>()) };
                }
                _ => {
                    unsafe { ptr::write(buffer, 10 as usize as _) };
                }
            }

            efi::Status::SUCCESS
        }

        let handles = boot_services.locate_handle(HandleSearchType::ByProtocol(&TestProtocol)).unwrap();
        assert_eq!(1, handles.len());
        assert_eq!(10, handles[0] as usize);
    }

    #[test]
    fn test_locate_handle_by_registry_notify() {
        let boot_services = boot_services!(
            locate_handle = efi_locate_handle,
            allocate_pool = efi_allocate_pool_use_box,
            free_pool = efi_free_pool_use_box
        );

        extern "efiapi" fn efi_locate_handle(
            search_type: efi::LocateSearchType,
            protocol: *mut efi::Guid,
            search_key: *mut c_void,
            buffer_size: *mut usize,
            buffer: *mut efi::Handle,
        ) -> efi::Status {
            assert_eq!(efi::BY_REGISTER_NOTIFY, search_type);
            assert_eq!(ptr::null_mut(), protocol);
            assert_eq!(10, search_key as usize);
            assert_ne!(ptr::null_mut(), buffer_size);

            match buffer {
                buffer if buffer.is_null() => {
                    assert_eq!(0, unsafe { ptr::read(buffer_size) });
                    unsafe { ptr::write(buffer_size, 1 * mem::size_of::<usize>()) };
                }
                _ => {
                    unsafe { ptr::write(buffer, 10 as usize as _) };
                }
            }

            efi::Status::SUCCESS
        }

        let handles = boot_services
            .locate_handle(HandleSearchType::ByRegisterNotify(unsafe { NonNull::new_unchecked(10 as usize as _) }))
            .unwrap();
        assert_eq!(1, handles.len());
        assert_eq!(10, handles[0] as usize);
    }

    #[test]
    #[should_panic = "Boot services function handle_protocol is not initialized."]
    fn test_handle_protocol_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.handle_protocol(ptr::null_mut(), &TestProtocol);
    }

    #[test]
    fn test_handle_protocol() {
        let boot_services = boot_services!(handle_protocol = efi_handle_protocol);

        extern "efiapi" fn efi_handle_protocol(
            handle: *mut c_void,
            protocol: *mut efi::Guid,
            interface: *mut *mut c_void,
        ) -> efi::Status {
            assert_eq!(1, handle as usize);
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_ne!(ptr::null_mut(), interface);
            let b = Box::new(12);
            unsafe { ptr::write(interface, b.into_raw_mut() as *mut _) };
            efi::Status::SUCCESS
        }

        let interface = boot_services.handle_protocol(1 as usize as _, &TestProtocol).unwrap();
        assert_eq!(12, *interface);
    }

    #[test]
    #[should_panic = "Boot services function locate_device_path is not initialized."]
    fn test_locate_device_path_not_init() {
        let boot_services = boot_services!();
        _ = unsafe { boot_services.locate_device_path(&TestProtocol, ptr::null_mut()) };
    }

    #[test]
    fn test_locate_device_path() {
        let boot_services = boot_services!(locate_device_path = efi_locate_device_path);

        extern "efiapi" fn efi_locate_device_path(
            protocol: *mut efi::Guid,
            device_path: *mut *mut device_path::Protocol,
            device: *mut efi::Handle,
        ) -> efi::Status {
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_eq!(1, device_path as usize);
            assert_ne!(ptr::null_mut(), device);
            unsafe { ptr::write(device, 12 as usize as _) };
            efi::Status::SUCCESS
        }

        let handle = unsafe { boot_services.locate_device_path(&TestProtocol, 1 as usize as _) }.unwrap();
        assert_eq!(12, handle as usize);
    }

    #[test]
    #[should_panic = "Boot services function open_protocol is not initialized."]
    fn test_open_protocol_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.open_protocol(ptr::null_mut(), &TestProtocol, ptr::null_mut(), ptr::null_mut(), 0);
    }

    #[test]
    fn test_open_protocol() {
        let boot_services = boot_services!(open_protocol = efi_open_protocol);

        extern "efiapi" fn efi_open_protocol(
            handle: efi::Handle,
            protocol: *mut efi::Guid,
            interface: *mut *mut c_void,
            agent_handle: efi::Handle,
            controller_handle: efi::Handle,
            attributes: u32,
        ) -> efi::Status {
            assert_eq!(1, handle as usize);
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_ne!(ptr::null_mut(), interface);
            assert_eq!(2, agent_handle as usize);
            assert_eq!(3, controller_handle as usize);
            assert_eq!(4, attributes);

            let b = Box::new(12);
            unsafe { ptr::write(interface, b.into_raw_mut() as _) };

            efi::Status::SUCCESS
        }

        let interface =
            boot_services.open_protocol(1 as usize as _, &TestProtocol, 2 as usize as _, 3 as usize as _, 4).unwrap();
        assert_eq!(12, *interface.unwrap())
    }

    #[test]
    #[should_panic = "Boot services function close_protocol is not initialized."]
    fn test_close_protocol_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.close_protocol(ptr::null_mut(), &TestProtocol, ptr::null_mut(), ptr::null_mut());
    }

    #[test]
    fn test_close_protocol() {
        let boot_services = boot_services!(close_protocol = efi_close_protocol);

        extern "efiapi" fn efi_close_protocol(
            handle: efi::Handle,
            protocol: *mut efi::Guid,
            agent_handle: efi::Handle,
            controller_handle: efi::Handle,
        ) -> efi::Status {
            assert_eq!(1, handle as usize);
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_eq!(2, agent_handle as usize);
            assert_eq!(3, controller_handle as usize);

            efi::Status::SUCCESS
        }

        _ = boot_services.close_protocol(1 as usize as _, &TestProtocol, 2 as usize as _, 3 as usize as _).unwrap();
    }

    #[test]
    #[should_panic = "Boot services function open_protocol_information is not initialized."]
    fn test_open_protocol_information_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.open_protocol_information(ptr::null_mut(), &TestProtocol);
    }

    #[test]
    fn test_open_protocol_information() {
        let boot_services = boot_services!(
            open_protocol_information = efi_open_protocol_information,
            free_pool = efi_free_pool_use_box
        );

        extern "efiapi" fn efi_open_protocol_information(
            handle: efi::Handle,
            protocol: *mut efi::Guid,
            entry_buffer: *mut *mut efi::OpenProtocolInformationEntry,
            entry_count: *mut usize,
        ) -> efi::Status {
            assert_eq!(1, handle as usize);
            assert_eq!(TEST_PROTOCOL_GUID, unsafe { ptr::read(protocol) });
            assert_ne!(ptr::null_mut(), entry_buffer);
            assert_ne!(ptr::null_mut(), entry_count);

            let buff = Box::new([efi::OpenProtocolInformationEntry {
                agent_handle: ptr::null_mut(),
                controller_handle: ptr::null_mut(),
                attributes: 10,
                open_count: 0,
            }])
            .into_raw_mut() as *mut OpenProtocolInformationEntry;

            unsafe {
                ptr::write(entry_buffer, buff);
                ptr::write(entry_count, 1)
            };

            efi::Status::SUCCESS
        }

        let info = boot_services.open_protocol_information(1 as usize as _, &TestProtocol).unwrap();
        assert_eq!(1, info.len());
        assert_eq!(10, info[0].attributes);
    }

    #[test]
    #[should_panic = "Boot services function connect_controller is not initialized."]
    fn test_connect_controller_not_init() {
        let boot_services = boot_services!();
        _ = unsafe { boot_services.connect_controller(ptr::null_mut(), vec![], ptr::null_mut(), false) };
    }

    #[test]
    fn test_connect_controller() {
        let boot_services = boot_services!(connect_controller = efi_connect_controller);

        extern "efiapi" fn efi_connect_controller(
            controller_handle: efi::Handle,
            driver_image_handles: *mut efi::Handle,
            remaining_device_path: *mut device_path::Protocol,
            recursive: Boolean,
        ) -> efi::Status {
            assert_eq!(1, controller_handle as usize);
            assert_eq!(ptr::null_mut(), driver_image_handles);
            assert_eq!(2, remaining_device_path as usize);
            assert_eq!(false, recursive.into());
            efi::Status::SUCCESS
        }

        _ = unsafe { boot_services.connect_controller(1 as usize as _, vec![], 2 as usize as _, false) }.unwrap();
    }

    #[test]
    fn test_connect_controller_with_image_handles() {
        let boot_services = boot_services!(connect_controller = efi_connect_controller);

        extern "efiapi" fn efi_connect_controller(
            controller_handle: efi::Handle,
            driver_image_handles: *mut efi::Handle,
            remaining_device_path: *mut device_path::Protocol,
            recursive: Boolean,
        ) -> efi::Status {
            assert_eq!(1, controller_handle as usize);
            assert_ne!(ptr::null_mut(), driver_image_handles);
            let image_handles = unsafe { slice::from_raw_parts(driver_image_handles as *const usize, 3) };
            assert_eq!([1, 2, 0], image_handles);
            assert_eq!(2, remaining_device_path as usize);
            assert_eq!(false, recursive.into());
            efi::Status::SUCCESS
        }

        _ = unsafe {
            boot_services.connect_controller(
                1 as usize as _,
                vec![1 as usize as _, 2 as usize as _],
                2 as usize as _,
                false,
            )
        }
        .unwrap();
    }

    #[test]
    #[should_panic = "Boot services function disconnect_controller is not initialized."]
    fn test_disconnect_controller_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.disconnect_controller(ptr::null_mut(), None, None);
    }

    #[test]
    fn test_disconnect_controller() {
        let boot_services = boot_services!(disconnect_controller = efi_disconnect_controller);

        extern "efiapi" fn efi_disconnect_controller(
            controller_handle: efi::Handle,
            driver_image_handle: efi::Handle,
            child_handle: efi::Handle,
        ) -> efi::Status {
            assert_eq!(1, controller_handle as usize);
            assert_eq!(ptr::null_mut(), driver_image_handle);
            assert_eq!(ptr::null_mut(), child_handle);
            efi::Status::SUCCESS
        }
        _ = boot_services.disconnect_controller(1 as usize as _, None, None).unwrap();
    }

    #[test]
    fn test_disconnect_controller_with_handles() {
        let boot_services = boot_services!(disconnect_controller = efi_disconnect_controller);

        extern "efiapi" fn efi_disconnect_controller(
            controller_handle: efi::Handle,
            driver_image_handle: efi::Handle,
            child_handle: efi::Handle,
        ) -> efi::Status {
            assert_eq!(1, controller_handle as usize);
            assert_eq!(2, driver_image_handle as usize);
            assert_eq!(3, child_handle as usize);
            efi::Status::SUCCESS
        }
        _ = boot_services.disconnect_controller(1 as usize as _, Some(2 as usize as _), Some(3 as usize as _)).unwrap();
    }

    #[test]
    #[should_panic = "Boot services function protocols_per_handle is not initialized."]
    fn test_protocol_per_handle_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.protocols_per_handle(ptr::null_mut());
    }

    #[test]
    fn test_protocol_per_handle() {
        let boot_services =
            boot_services!(protocols_per_handle = efi_protocol_per_handle, free_pool = efi_free_pool_use_box);

        extern "efiapi" fn efi_protocol_per_handle(
            handle: efi::Handle,
            protocol_buffer: *mut *mut *mut efi::Guid,
            protocol_buffer_count: *mut usize,
        ) -> efi::Status {
            assert_eq!(1, handle as usize);
            assert_ne!(ptr::null_mut(), protocol_buffer);
            assert_ne!(ptr::null_mut(), protocol_buffer_count);

            #[allow(unused_allocation)]
            let buff = Box::new(ptr::addr_of!(TEST_PROTOCOL_GUID) as *mut efi::Guid).into_raw_mut();

            unsafe {
                ptr::write(protocol_buffer, buff);
                ptr::write(protocol_buffer_count, 1);
            }

            efi::Status::SUCCESS
        }

        let protocols = boot_services.protocols_per_handle(1 as usize as _).unwrap();
        assert_eq!(1, protocols.len());
        assert_eq!(TEST_PROTOCOL_GUID, *protocols[0]);
    }

    #[test]
    #[should_panic = "Boot services function locate_protocol is not initialized."]
    fn test_locate_protocol_not_init() {
        let boot_services = boot_services!();
        _ = unsafe { boot_services.locate_protocol_2(&TestProtocol, None) };
    }

    #[test]
    fn test_locate_protocol() {
        let boot_services = boot_services!(locate_protocol = efi_locate_protocol);

        static PROTOCOL_INTERFACE: u32 = 10;

        extern "efiapi" fn efi_locate_protocol(
            protocol_guid: *mut efi::Guid,
            registration: *mut c_void,
            interface: *mut *mut c_void,
        ) -> efi::Status {
            assert!(!protocol_guid.is_null());
            assert!(registration.is_null());
            assert!(!interface.is_null());
            assert_eq!(unsafe { ptr::read(protocol_guid) }, TEST_PROTOCOL_GUID);
            unsafe { ptr::write(interface, &PROTOCOL_INTERFACE as *const u32 as *mut u32 as *mut c_void) };
            efi::Status::SUCCESS
        }

        let protocol = unsafe { boot_services.locate_protocol_2(&TestProtocol, None) }.unwrap();
        assert_eq!(PROTOCOL_INTERFACE, *protocol);
    }

    #[test]
    fn test_locate_protocol_marker() {
        let boot_services = boot_services!(locate_protocol = efi_locate_protocol);

        extern "efiapi" fn efi_locate_protocol(
            protocol_guid: *mut efi::Guid,
            registration: *mut c_void,
            interface: *mut *mut c_void,
        ) -> efi::Status {
            assert!(!protocol_guid.is_null());
            assert!(registration.is_null());
            assert!(!interface.is_null());
            assert_eq!(unsafe { ptr::read(protocol_guid) }, TEST_PROTOCOL_GUID);
            efi::Status::SUCCESS
        }

        unsafe { boot_services.locate_protocol_marker(&TestProtocolMarker, None) }.unwrap();
    }

    #[test]
    #[should_panic = "Marker interface are not supported by locate_protocol, use locate_protocol_marker instead."]
    fn test_locate_protocol_marker_with_locate_protocol() {
        let boot_services = boot_services!(locate_protocol = efi_locate_protocol);

        extern "efiapi" fn efi_locate_protocol(
            _protocol_guid: *mut efi::Guid,
            _registration: *mut c_void,
            _interface: *mut *mut c_void,
        ) -> efi::Status {
            efi::Status::SUCCESS
        }

        unsafe { boot_services.locate_protocol_2(&TestProtocolMarker, None) }.unwrap();
    }

    #[test]
    #[should_panic = "Boot services function load_image is not initialized."]
    fn test_load_image_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.load_image(false, ptr::null_mut(), ptr::null_mut(), None);
    }

    #[test]
    fn test_load_image() {
        let boot_services = boot_services!(load_image = efi_load_image);

        extern "efiapi" fn efi_load_image(
            boot_policy: Boolean,
            parent_image_handler: *mut c_void,
            device_path: *mut device_path::Protocol,
            source_buffer: *mut c_void,
            source_size: usize,
            image_handler: *mut *mut c_void,
        ) -> efi::Status {
            assert_eq!(true, boot_policy.into());
            assert_eq!(1, parent_image_handler as usize);
            assert_eq!(2, device_path as usize);
            assert_eq!(5, source_size);
            let source = unsafe { slice::from_raw_parts(source_buffer as *mut u8, source_size) };
            assert_eq!(&[1_u8, 2, 3, 4, 5], source);
            unsafe {
                ptr::write(image_handler, 3 as usize as _);
            }
            efi::Status::SUCCESS
        }

        let image_handle = boot_services
            .load_image(
                true,
                1 as usize as *mut c_void,
                2 as usize as *mut device_path::Protocol,
                Some(&[1_u8, 2, 3, 4, 5]),
            )
            .unwrap();

        assert_eq!(3, image_handle as usize);
    }

    #[test]
    fn test_load_image_from_source() {
        let boot_services = boot_services!(load_image = efi_load_image);

        extern "efiapi" fn efi_load_image(
            boot_policy: Boolean,
            parent_image_handler: *mut c_void,
            device_path: *mut device_path::Protocol,
            source_buffer: *mut c_void,
            source_size: usize,
            _image_handler: *mut *mut c_void,
        ) -> efi::Status {
            assert_eq!(false, boot_policy.into());
            assert_eq!(1, parent_image_handler as usize);
            assert_eq!(2, device_path as usize);
            assert_eq!(5, source_size);
            let source = unsafe { slice::from_raw_parts(source_buffer as *mut u8, source_size) };
            assert_eq!(&[1_u8, 2, 3, 4, 5], source);
            efi::Status::SUCCESS
        }

        _ = boot_services.load_image_from_source(1 as usize as _, 2 as usize as _, &[1_u8, 2, 3, 4, 5])
    }

    #[test]
    fn test_load_image_from_file() {
        let boot_services = boot_services!(load_image = efi_load_image);

        extern "efiapi" fn efi_load_image(
            boot_policy: Boolean,
            parent_image_handler: *mut c_void,
            device_path: *mut device_path::Protocol,
            source_buffer: *mut c_void,
            source_size: usize,
            _image_handler: *mut *mut c_void,
        ) -> efi::Status {
            assert_eq!(false, boot_policy.into());
            assert_eq!(1, parent_image_handler as usize);
            assert_eq!(2, device_path as usize);
            assert_eq!(ptr::null_mut(), source_buffer);
            assert_eq!(0, source_size);
            efi::Status::SUCCESS
        }

        _ = boot_services.load_image_from_file(1 as usize as _, NonNull::new(2 as usize as _).unwrap());
    }

    #[test]
    #[should_panic = "Boot services function start_image is not initialized."]
    fn test_start_image_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.start_image(ptr::null_mut());
    }

    #[test]
    fn test_start_image() {
        let boot_services = boot_services!(start_image = efi_start_image);

        extern "efiapi" fn efi_start_image(
            image_handle: efi::Handle,
            exit_data_size: *mut usize,
            exit_data: *mut *mut Char16,
        ) -> efi::Status {
            assert_eq!(1, image_handle as usize);
            assert_ne!(ptr::null_mut(), exit_data_size);
            assert_ne!(ptr::null_mut(), exit_data);
            efi::Status::SUCCESS
        }

        _ = boot_services.start_image(1 as usize as _).unwrap();
    }

    #[test]
    #[should_panic = "Boot services function unload_image is not initialized."]
    fn test_unload_image_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.unload_image(ptr::null_mut());
    }

    #[test]
    fn test_unload_image() {
        let boot_services = boot_services!(unload_image = efi_unload_image);

        extern "efiapi" fn efi_unload_image(image_handle: efi::Handle) -> efi::Status {
            assert_eq!(1, image_handle as usize);
            efi::Status::SUCCESS
        }

        _ = boot_services.unload_image(1 as usize as _);
    }

    #[test]
    #[should_panic = "Boot services function exit is not initialized."]
    fn test_exit_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.exit(ptr::null_mut(), efi::Status::SUCCESS, None);
    }

    #[test]
    fn test_exit() {
        let boot_services = boot_services!(exit = efi_exit);

        extern "efiapi" fn efi_exit(
            image_handle: efi::Handle,
            exit_status: efi::Status,
            exit_data_size: usize,
            exit_data: *mut u16,
        ) -> efi::Status {
            assert_eq!(1, image_handle as usize);
            assert_eq!(efi::Status::SUCCESS, exit_status);
            assert_eq!(0, exit_data_size);
            assert_eq!(ptr::null_mut(), exit_data);
            efi::Status::SUCCESS
        }

        _ = boot_services.exit(1 as usize as _, efi::Status::SUCCESS, None).unwrap();
    }

    #[test]
    #[should_panic = "Boot services function exit_boot_services is not initialized."]
    fn test_exit_boot_services_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.exit_boot_services(ptr::null_mut(), 0);
    }

    #[test]
    fn test_exit_boot_services() {
        let boot_services = boot_services!(exit_boot_services = efi_exit_boot_services);

        extern "efiapi" fn efi_exit_boot_services(image_handle: efi::Handle, map_key: usize) -> efi::Status {
            assert_eq!(1, image_handle as usize);
            assert_eq!(2, map_key);
            efi::Status::SUCCESS
        }

        _ = boot_services.exit_boot_services(1 as usize as _, 2).unwrap();
    }

    #[test]
    fn test_get_memory_map() {
        let boot_services = boot_services!(
            get_memory_map = efi_get_memory_map,
            allocate_pool = efi_allocate_pool_use_box,
            free_pool = efi_free_pool_use_box
        );

        extern "efiapi" fn efi_get_memory_map(
            memory_map_size: *mut usize,
            memory_map: *mut efi::MemoryDescriptor,
            _map_key: *mut usize,
            descriptor_size: *mut usize,
            descriptor_version: *mut u32,
        ) -> efi::Status {
            if memory_map_size.is_null() {
                return efi::Status::INVALID_PARAMETER;
            }

            let memory_map_size_value = unsafe { *memory_map_size };
            if memory_map_size_value == 0 {
                unsafe { ptr::write(memory_map_size, 0x400) };
                return efi::Status::BUFFER_TOO_SMALL;
            }

            unsafe {
                (*memory_map).physical_start = 0xffffffffaaaabbbb;
                *descriptor_size = mem::size_of::<efi::MemoryDescriptor>();
                *descriptor_version = 1;
            }
            efi::Status::SUCCESS
        }

        let status = boot_services.get_memory_map();

        match status {
            Ok(memory_map) => {
                assert_eq!(memory_map.map_key, 0);
                assert_eq!(memory_map.descriptor_version, 1);
                assert_eq!(memory_map.descriptors[0].physical_start, 0xffffffffaaaabbbb);
            }
            Err((status, _)) => {
                assert!(false, "Error: {:?}", status);
            }
        }
    }

    #[test]
    #[should_panic = "Boot services function set_watchdog_timer is not initialized."]
    fn test_set_watchdog_timer_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.set_watchdog_timer(0)
    }

    #[test]
    fn test_set_watchdog_timer() {
        let boot_services = boot_services!(set_watchdog_timer = efi_set_watchdog_timer);

        extern "efiapi" fn efi_set_watchdog_timer(
            timeout: usize,
            watchdog_code: u64,
            data_size: usize,
            watchdog_data: *mut u16,
        ) -> efi::Status {
            assert_eq!(10, timeout);
            assert_eq!(0, watchdog_code);
            assert_eq!(0, data_size);
            assert_eq!(ptr::null_mut(), watchdog_data);
            efi::Status::SUCCESS
        }

        boot_services.set_watchdog_timer(10).unwrap();
    }

    #[test]
    #[should_panic = "Boot services function stall is not initialized."]
    fn test_stall_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.stall(0);
    }

    #[test]
    fn test_stall() {
        let boot_services = boot_services!(stall = efi_stall);
        extern "efiapi" fn efi_stall(microsecondes: usize) -> efi::Status {
            assert_eq!(10, microsecondes);
            efi::Status::SUCCESS
        }
        let status = boot_services.stall(10);
        assert_eq!(Ok(()), status);
    }

    #[test]
    #[should_panic = "Boot services function copy_mem is not initialized."]
    fn test_copy_mem_not_init() {
        let boot_services = boot_services!();
        let mut dest = 0;
        let src = 0;
        boot_services.copy_mem(&mut dest, &src);
    }

    #[test]
    #[allow(static_mut_refs)]
    fn test_copy_mem() {
        let boot_services = boot_services!(copy_mem = efi_copy_mem);

        static A: [i32; 5] = [1, 2, 3, 4, 5];
        static mut B: [i32; 5] = [0; 5];

        extern "efiapi" fn efi_copy_mem(dest: *mut c_void, src: *mut c_void, length: usize) {
            assert_eq!(unsafe { ptr::addr_of!(B) } as usize, dest as usize);
            assert_eq!(ptr::addr_of!(A) as usize, src as usize);
            assert_eq!(5 * mem::size_of::<i32>(), length);
        }
        boot_services.copy_mem(unsafe { &mut B }, &A);
    }

    #[test]
    #[should_panic = "Boot services function set_mem is not initialized."]
    fn test_set_mem_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.set_mem(&mut [0], 0);
    }

    #[test]
    #[allow(static_mut_refs)]
    fn test_set_mem() {
        let boot_services = boot_services!(set_mem = efi_set_mem);

        static mut BUFFER: [u8; 16] = [0; 16];

        extern "efiapi" fn efi_set_mem(buffer: *mut c_void, size: usize, value: u8) {
            assert_eq!(unsafe { ptr::addr_of!(BUFFER) } as usize, buffer as usize);
            assert_eq!(16, size);
            assert_eq!(8, value);
        }
        _ = boot_services.set_mem(unsafe { &mut BUFFER }, 8);
    }

    #[test]
    #[should_panic = "Boot services function get_next_monotonic_count is not initialized."]
    fn test_get_next_monotonic_count_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.get_next_monotonic_count();
    }

    #[test]
    fn test_get_next_monotonic_count() {
        let boot_services = boot_services!(get_next_monotonic_count = efi_get_next_monotonic_count);
        extern "efiapi" fn efi_get_next_monotonic_count(count: *mut u64) -> efi::Status {
            unsafe { ptr::write(count, 89) };
            efi::Status::SUCCESS
        }
        let status = boot_services.get_next_monotonic_count().unwrap();
        assert_eq!(89, status);
    }

    #[test]
    #[should_panic = "Boot services function install_configuration_table is not initialized."]
    fn test_install_configuration_table_not_init() {
        let boot_services = boot_services!();
        let table = Box::new(());
        _ = boot_services.install_configuration_table(&efi::Guid::from_bytes(&[0; 16]), table);
    }

    #[test]
    fn test_install_configuration_table() {
        let boot_services = boot_services!(install_configuration_table = efi_install_configuration_table);

        static GUID: efi::Guid = efi::Guid::from_bytes(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        static mut TABLE: i32 = 10;

        extern "efiapi" fn efi_install_configuration_table(guid: *mut efi::Guid, table: *mut c_void) -> efi::Status {
            assert_eq!(ptr::addr_of!(GUID) as usize, guid as usize);
            assert_eq!(unsafe { ptr::addr_of!(TABLE) } as usize, table as usize);
            assert_eq!(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], unsafe { ptr::read(guid) }.as_bytes());
            assert_eq!(10, unsafe { ptr::read(table as *mut i32) });
            efi::Status::SUCCESS
        }

        #[allow(static_mut_refs)]
        boot_services.install_configuration_table(&GUID, unsafe { &mut TABLE }).unwrap();
    }

    #[test]
    #[should_panic = "Boot services function calculate_crc32 is not initialized."]
    fn test_calculate_crc32_not_init() {
        let boot_services = boot_services!();
        _ = boot_services.calculate_crc_32(&[0]);
    }

    #[test]
    fn test_calculate_crc32() {
        let boot_services = boot_services!(calculate_crc32 = efi_calculate_crc32);

        static BUFFER: [u8; 16] = [0; 16];

        extern "efiapi" fn efi_calculate_crc32(
            buffer_ptr: *mut c_void,
            buffer_size: usize,
            crc: *mut u32,
        ) -> efi::Status {
            unsafe {
                assert_eq!(ptr::addr_of!(BUFFER) as usize, buffer_ptr as usize);
                assert_eq!(BUFFER.len(), buffer_size);
                ptr::write(crc, 10)
            }
            efi::Status::SUCCESS
        }

        let crc = boot_services.calculate_crc_32(&BUFFER).unwrap();
        assert_eq!(10, crc);
    }
}
