//! This module defined every struct related to event in boot services.

use alloc::boxed::Box;
use core::{
  mem::ManuallyDrop,
  ops::{self, Deref},
  pin::Pin,
  ptr,
};

use r_efi::efi;

/// Function signature for event notify function.
pub type EventNotifyCallback<T> = extern "efiapi" fn(efi::Event, T);

/// Trait implemented for every type a context pointer can be.
///
/// <div class="warning">
///
/// This should be implemented **only** on type that have the same memory layout as `*mut T` and that can be recreated with [`core::mem::transmute`].
///
/// </div>
pub unsafe trait EventCtxMutPtr {
  /// The contntext type. It need to be size to prevent any fat pointer wich does not have `*mut T` memory layout.
  type Ctx: Sized + 'static;
  /// This function convert the type into its raw form.
  fn into_raw_mut(self) -> *mut Self::Ctx;
}

unsafe impl<T: Sized + Sync + 'static> EventCtxMutPtr for &'static T {
  type Ctx = T;
  fn into_raw_mut(self) -> *mut Self::Ctx {
    ptr::from_ref(self) as *mut _
  }
}

unsafe impl<T: Sized + Sync + 'static> EventCtxMutPtr for &'static mut T {
  type Ctx = T;
  fn into_raw_mut(self) -> *mut Self::Ctx {
    ptr::from_mut(self) as *mut _
  }
}

unsafe impl<T: Sized + 'static> EventCtxMutPtr for Box<T> {
  type Ctx = T;
  fn into_raw_mut(self) -> *mut Self::Ctx {
    ptr::from_mut(Box::leak(self))
  }
}

unsafe impl<C: EventCtxMutPtr<Ctx = T>, T: Sized + 'static> EventCtxMutPtr for Option<C> {
  type Ctx = T;
  fn into_raw_mut(self) -> *mut Self::Ctx {
    self.map(C::into_raw_mut).unwrap_or(ptr::null_mut())
  }
}

unsafe impl<C: EventCtxMutPtr<Ctx = T>, T: Sized + 'static> EventCtxMutPtr for ManuallyDrop<C> {
  type Ctx = T;
  fn into_raw_mut(self) -> *mut Self::Ctx {
    C::into_raw_mut(ManuallyDrop::into_inner(self))
  }
}

unsafe impl<C: EventCtxMutPtr<Ctx = T> + Deref<Target = T>, T: Sized + Unpin + 'static> EventCtxMutPtr for Pin<C> {
  type Ctx = T;
  fn into_raw_mut(self) -> *mut Self::Ctx {
    C::into_raw_mut(Pin::into_inner(self))
  }
}

/// The type of time that is specified in TriggerTime. See the timer delay types in “Related Definitions.”
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum EventTimerType {
  /// The event’s timer setting is to be cancelled and no timer trigger is to be set.
  /// TriggerTime is ignored when canceling a timer.
  Cancel = efi::TIMER_CANCEL,

  /// The event is to be signaled periodically at TriggerTime intervals from the current time.
  /// This is the only timer trigger Type for which the event timer does not need to be reset for each notification.
  /// All other timer trigger types are “one shot.”
  Periodic = efi::TIMER_PERIODIC,

  /// The event is to be signaled in TriggerTime 100ns units.
  Relative = efi::TIMER_RELATIVE,
}

impl Into<u32> for EventTimerType {
  fn into(self) -> u32 {
    match self {
      t => t as u32,
    }
  }
}

/// Type of event to create and its mode and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct EventType(u32);

impl EventType {
  /// The event is a timer event and may be passed to [`BootServices::set_timer`](super::BootServices::set_timer).
  /// Note that timers only function during boot services time.
  pub const TIMER: EventType = EventType(efi::EVT_TIMER);

  /// The event is allocated from runtime memory.
  /// If an event is to be signaled after the call to [`BootServices.exit_boot_services`] the event’s data structure and notification function need to be allocated from runtime memory.
  /// For more information, see
  /// <a href="https://uefi.org/specs/UEFI/2.10/08_Services_Runtime_Services.html#setvirtualaddressmap" target="_blank">
  ///   SetVirtualAddressMap()
  /// </a> .
  pub const RUNTIME: EventType = EventType(efi::EVT_RUNTIME);

  /// If an event of this type is not already in the signaled state,
  /// then the event’s NotificationFunction will be queued at the event’s NotifyTpl whenever the event is being waited
  /// on via [`BootServices::wait_for_event`](super::BootServices::wait_for_event) or [`BootServices::check_event`](super::BootServices::check_event).
  pub const NOTIFY_WAIT: EventType = EventType(efi::EVT_NOTIFY_WAIT);

  /// The event’s NotifyFunction is queued whenever the event is signaled.
  pub const NOTIFY_SIGNAL: EventType = EventType(efi::EVT_NOTIFY_SIGNAL);

  /// This event is of type [Self::NOTIFY_SIGNAL].
  /// It should not be combined with any other event types.
  /// This event type is functionally equivalent to the `EFI_EVENT_GROUP_EXIT_BOOT_SERVICES` event group.
  /// Refer to `EFI_EVENT_GROUP_EXIT_BOOT_SERVICES` event group description in [`BootServices::create_event_ex`](super::BootServices::create_event_ex) section below for additional details.
  pub const SIGNAL_EXIT_BOOT_SERVICES: EventType = EventType(efi::EVT_SIGNAL_EXIT_BOOT_SERVICES);

  /// The event is to be notified by the system when `SetVirtualAddressMap()` is performed.
  /// This event type is a composite of [`Self::NOTIFY_SIGNAL`], [`Self::RUNTIME`], and [`Self::RUNTIME`] and should not be combined with any other event types.
  pub const SIGNAL_VIRTUAL_ADDRESS_CHANGE: EventType = EventType(efi::EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE);
}

impl ops::BitOr for EventType {
  type Output = EventType;

  fn bitor(self, rhs: Self) -> Self::Output {
    EventType(self.0 | rhs.0)
  }
}

impl ops::BitOrAssign for EventType {
  fn bitor_assign(&mut self, rhs: Self) {
    self.0 |= rhs.0;
  }
}

impl Into<u32> for EventType {
  fn into(self) -> u32 {
    self.0
  }
}
