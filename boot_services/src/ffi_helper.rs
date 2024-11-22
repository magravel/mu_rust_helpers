use alloc::boxed::Box;
use core::{
    ffi::c_void,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ops::Deref,
    ptr,
};

#[derive(Clone, Copy)]
pub struct PtrMetadata<'a, T> {
    pub ptr_value: usize,
    _container: PhantomData<&'a T>,
}

impl<'a, R: CPtr<'a, Type = T>, T> PtrMetadata<'a, R> {
    pub unsafe fn into_original_ptr(self) -> R {
        mem::transmute_copy(&self.ptr_value)
    }
}

pub unsafe trait CPtr<'a>: Sized {
    type Type: Sized;

    fn as_ptr(&self) -> *const Self::Type;

    fn into_ptr(self) -> *const Self::Type {
        let this = ManuallyDrop::new(self);
        this.as_ptr()
    }

    fn metadata(&self) -> PtrMetadata<'a, Self> {
        PtrMetadata { ptr_value: self.as_ptr() as usize, _container: PhantomData }
    }
}
pub unsafe trait CMutPtr<'a>: CPtr<'a> {
    fn as_mut_ptr(&mut self) -> *mut Self::Type {
        <Self as CPtr>::as_ptr(self) as *mut _
    }

    fn into_mut_ptr(self) -> *mut Self::Type {
        let mut this = ManuallyDrop::new(self);
        this.as_mut_ptr()
    }
}

pub unsafe trait CRef<'a>: CPtr<'a> {
    fn as_ref(&self) -> &Self::Type {
        unsafe { self.as_ptr().as_ref().unwrap() }
    }
}

pub unsafe trait CMutRef<'a>: CRef<'a> + CMutPtr<'a> {
    fn as_mut(&mut self) -> &mut Self::Type {
        unsafe { self.as_mut_ptr().as_mut().unwrap() }
    }
}

// &T
unsafe impl<'a, T> CPtr<'a> for &'a T {
    type Type = T;

    fn as_ptr(&self) -> *const Self::Type {
        *self as *const _
    }
}
unsafe impl<'a, T> CRef<'a> for &'a T {}

// &mut T
unsafe impl<'a, T> CPtr<'a> for &'a mut T {
    type Type = T;

    fn as_ptr(&self) -> *const Self::Type {
        *self as *const _
    }
}
unsafe impl<'a, T> CRef<'a> for &'a mut T {}
unsafe impl<'a, T> CMutPtr<'a> for &'a mut T {}
unsafe impl<'a, T> CMutRef<'a> for &'a mut T {}

// Box<T>
unsafe impl<'a, T> CPtr<'a> for Box<T> {
    type Type = T;

    fn as_ptr(&self) -> *const Self::Type {
        AsRef::as_ref(self) as *const _
    }
}
unsafe impl<'a, T> CRef<'a> for Box<T> {}
unsafe impl<'a, T> CMutPtr<'a> for Box<T> {}
unsafe impl<'a, T> CMutRef<'a> for Box<T> {}

// ()
unsafe impl CPtr<'static> for () {
    type Type = c_void;

    fn as_ptr(&self) -> *const Self::Type {
        ptr::null()
    }
}

unsafe impl CMutPtr<'static> for () {
    fn as_mut_ptr(&mut self) -> *mut Self::Type {
        ptr::null_mut()
    }
}

// Option<T>
unsafe impl<'a, R: CPtr<'a, Type = T>, T> CPtr<'a> for Option<R> {
    type Type = T;

    fn as_ptr(&self) -> *const Self::Type {
        self.as_ref().map_or(ptr::null(), |p| p.as_ptr())
    }
}
unsafe impl<'a, R: CMutPtr<'a, Type = T>, T> CMutPtr<'a> for Option<R> {}

// ManuallyDrop<T>
unsafe impl<'a, R: CPtr<'a, Type = T>, T> CPtr<'a> for ManuallyDrop<R> {
    type Type = T;

    fn as_ptr(&self) -> *const Self::Type {
        <R as CPtr>::as_ptr(self.deref())
    }
}
unsafe impl<'a, R: CMutPtr<'a, Type = T>, T> CMutPtr<'a> for ManuallyDrop<R> {}
unsafe impl<'a, R: CRef<'a, Type = T>, T> CRef<'a> for ManuallyDrop<R> {}
unsafe impl<'a, R: CMutRef<'a, Type = T>, T> CMutRef<'a> for ManuallyDrop<R> {}

#[cfg(test)]
mod test {
    use core::ptr;

    use super::*;

    #[test]
    fn test_ref() {
        let mut foo = 10;
        let ptr = ptr::addr_of!(foo);

        assert_eq!(ptr, (&foo).as_ptr());
        assert_eq!(ptr, (&mut foo).as_mut_ptr());

        assert_eq!(ptr, (&foo).as_ref() as *const _);
        assert_eq!(ptr, (&mut foo).as_mut() as *const _);

        assert_eq!(ptr, (&mut foo).into_ptr());
        let mut foo = 10;
        let ptr = ptr::addr_of!(foo);
        assert_eq!(ptr, (&mut foo).into_mut_ptr());
    }

    #[test]
    fn test_box() {
        let b = Box::new(10);
        let b_ptr = ptr::from_ref(<Box<_> as AsRef<_>>::as_ref(&b));

        assert_eq!(b_ptr, CPtr::as_ptr(&b));
        assert_eq!(b_ptr, CPtr::into_ptr(b));

        // Box should leak with into_ptr
        let mut b = unsafe { Box::from_raw(b_ptr as *mut i32) };
        assert_eq!(&10, <Box<_> as AsRef<_>>::as_ref(&b));

        assert_eq!(b_ptr, CMutPtr::as_mut_ptr(&mut b));
        assert_eq!(b_ptr, CMutPtr::into_mut_ptr(b));
    }

    #[test]
    fn test_unit_type() {
        assert_eq!(ptr::null(), ().as_ptr());
        assert_eq!(ptr::null_mut(), ().as_mut_ptr());
    }

    #[test]
    fn test_option() {
        assert_eq!(ptr::null(), (Option::<Box<i32>>::None).as_ptr());
        assert_eq!(ptr::null_mut(), (Option::<Box<i32>>::None).as_mut_ptr());

        let b = Box::new(10);
        let ptr = b.as_ptr();
        assert_eq!(ptr, Some(b).as_ptr());

        let b = Box::new(10);
        let ptr = b.as_ptr();
        assert_eq!(ptr, Some(b).as_mut_ptr());

        let b = Box::new(10);
        let ptr = b.as_ptr();
        assert_eq!(ptr, Some(b).into_ptr());

        let b = Box::new(10);
        let ptr = b.as_ptr();
        assert_eq!(ptr, Some(b).into_mut_ptr());
    }

    #[test]
    fn test_manually_drop() {
        let b = Box::new(10);
        let ptr = b.as_ptr();
        let mut mdb = ManuallyDrop::new(b);
        assert_eq!(ptr, mdb.as_ptr());
        assert_eq!(ptr, mdb.as_mut_ptr());
        assert_eq!(ptr, mdb.into_ptr());

        let mdb = ManuallyDrop::new(unsafe { Box::from_raw(ptr as *mut i32) });
        assert_eq!(ptr, mdb.into_mut_ptr());

        assert_eq!(ptr::null(), ManuallyDrop::new(()).as_ptr());
        assert_eq!(ptr::null_mut(), ManuallyDrop::new(()).as_mut_ptr());
    }
}
