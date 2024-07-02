use core::ops;
use std::{ffi::c_void, ops::Deref, pin::Pin, ptr};

use r_efi::efi::{Event, EVT_NOTIFY_SIGNAL, EVT_NOTIFY_WAIT, EVT_RUNTIME, EVT_SIGNAL_EXIT_BOOT_SERVICES, EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE, EVT_TIMER, TIMER_CANCEL, TIMER_PERIODIC, TIMER_RELATIVE};

pub type EventNotifyCallback<T> = extern "efiapi" fn(Event, T);

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct NoContext(*mut c_void);

/// Only implement this trait on struct that can be recreated from a ptr with mem::transmute.
pub unsafe trait EventCtxMutPtr<T>
where
  T: Sized,
  Self: Sized,
{
  type FFIType;
  // we need this function because the transmute does not works with generic of because the compiler does not know the real size of the struct.
  fn into_raw_mut(self) -> *mut T;
}

unsafe impl<T: Sized + Unpin + Sync> EventCtxMutPtr<T> for &'static T {
  type FFIType = Self;
  fn into_raw_mut(self) -> *mut T {
    ptr::from_ref(self) as *mut T
  }
}

unsafe impl<T: Sized + Unpin + Sync> EventCtxMutPtr<T> for &'static mut T {
  type FFIType = Self;
  fn into_raw_mut(self) -> *mut T {
    ptr::from_mut(self) as *mut T
  }
}

unsafe impl<T: Sized + Send> EventCtxMutPtr<T> for Box<T> {
  type FFIType = Self;
  fn into_raw_mut(self) -> *mut T {
    ptr::from_mut(Box::leak(self))
  }
}

unsafe impl<C: EventCtxMutPtr<T, FFIType = C> + 'static, T: Sized + 'static> EventCtxMutPtr<T> for Option<C> {
  type FFIType = Self;
  fn into_raw_mut(self) -> *mut T {
    self.map(C::into_raw_mut).unwrap_or(ptr::null_mut())
  }
}

unsafe impl<C: EventCtxMutPtr<T, FFIType = C> + 'static + Deref<Target = T>, T: Sized + Unpin + 'static>
  EventCtxMutPtr<T> for Pin<C>
{
  type FFIType = Self;
  fn into_raw_mut(self) -> *mut T {
    C::into_raw_mut(Pin::into_inner(self))
  }
}

unsafe impl EventCtxMutPtr<c_void> for () {
  type FFIType = NoContext;
  fn into_raw_mut(self) -> *mut c_void {
    ptr::null_mut()
  }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum EventTimerType {
  Cancel = TIMER_CANCEL,
  Periodic = TIMER_PERIODIC,
  Relative = TIMER_RELATIVE,
}

impl Into<u32> for EventTimerType {
    fn into(self) -> u32 {
      match self {
         t => t as u32  
      }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(transparent)]
pub struct EventType(u32);

impl EventType {
  pub const TIMER: EventType = EventType(EVT_TIMER);
  ///
  pub const RUNTIME: EventType = EventType(EVT_RUNTIME);
  ///
  pub const NOTIFY_WAIT: EventType = EventType(EVT_NOTIFY_WAIT);
  ///
  pub const NOTIFY_SIGNAL: EventType = EventType(EVT_NOTIFY_SIGNAL);
  ///
  pub const SIGNAL_EXIT_BOOT_SERVICES: EventType = EventType(EVT_SIGNAL_EXIT_BOOT_SERVICES);
  ///
  pub const SIGNAL_VIRTUAL_ADDRESS_CHANGE: EventType = EventType(EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE);

  pub fn is(&self, event_type: EventType) -> bool {
    self.0 & event_type.0 == self.0
  }
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

#[cfg(test)]
mod test {
  use super::EventType;

  #[test]
  fn t() {
    for (t, s) in &[
      (EventType::TIMER, "TIMER"),
      (EventType::RUNTIME, "RUNTIME"),
      (EventType::NOTIFY_WAIT, "NOTIFY_WAIT"),
      (EventType::NOTIFY_SIGNAL, "NOTIFY_SIGNAL"),
      (EventType::SIGNAL_EXIT_BOOT_SERVICES, "SIGNAL_EXIT_BOOT_SERVICES"),
      (EventType::SIGNAL_VIRTUAL_ADDRESS_CHANGE, "SIGNAL_VIRTUAL_ADDRESS_CHANGE"),
    ] {
      println!("{:032b} {}", t.0, s);
    }

    println!();
    let _a = EventType::RUNTIME | EventType::SIGNAL_VIRTUAL_ADDRESS_CHANGE;
    let a = EventType::TIMER | EventType::RUNTIME;
    println!("{:032b}", a.0);
  }
}
