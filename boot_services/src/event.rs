use core::ops;
use std::{ffi::c_void, ptr};

use r_efi::efi::Event;

pub type EventCallback<C> = extern "efiapi" fn(Event, C);

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct NoContext(*mut c_void);

/// Only implement this trait on struct that can be recreated from a ptr with mem::transmute.
pub unsafe trait EventCtxMutPtr<T: Sized>
where
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

unsafe impl<T: Sized + Send> EventCtxMutPtr<T> for Box<T> {
  type FFIType = Self;
  fn into_raw_mut(self) -> *mut T {
    ptr::from_mut(Box::leak(self))
  }
}

unsafe impl EventCtxMutPtr<c_void> for () {
  type FFIType = NoContext;
  fn into_raw_mut(self) -> *mut c_void {
    ptr::null_mut()
  }
}

#[derive(Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct EventType(u32);

impl EventType {
  ///
  pub const TIMER: EventType = EventType(0x80000000u32);
  ///
  pub const RUNTIME: EventType = EventType(0x40000000u32);
  ///
  pub const NOTIFY_WAIT: EventType = EventType(0x00000100u32);
  ///
  pub const NOTIFY_SIGNAL: EventType = EventType(0x00000200u32);
  ///
  pub const SIGNAL_EXIT_BOOT_SERVICES: EventType = EventType(0x00000201u32);
  ///
  pub const SIGNAL_VIRTUAL_ADDRESS_CHANGE: EventType = EventType(0x60000202u32);

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
