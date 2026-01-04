use core::fmt::Debug;
use core::mem;
use nalgebra::Scalar;
use nalgebra_glm::TVec2;
use num_traits::{PrimInt, Unsigned};

pub struct Slots<T, S: PrimInt + Unsigned + Scalar + Into<usize> + core::iter::Step + Debug, const N: usize> {
    slots: [Option<T>; N],
    free: heapless::Vec<S, N>,
}

impl<T, S: PrimInt + Unsigned + Scalar + Into<usize> + core::iter::Step + Debug, const N: usize> Slots<T, S, N> {
    pub fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
            free: heapless::Vec::from_iter(S::zero()..S::from(N).unwrap()),
        }
    }
    
    pub unsafe fn init(&mut self) {
        self.slots.fill_with(|| None);
        self.free.set_len(0);
        self.free.extend(S::zero()..S::from(N).unwrap());
    }

    pub fn push(&mut self, item: T) -> Result<S, ()> {
        let slot = self.free.pop().ok_or(())?;
        self.slots[<S as Into<usize>>::into(slot)] = Some(item);
        Ok(slot)
    }

    pub fn pop(&mut self, slot: S) -> Option<T> {
        if let Some(item) = self.slots[<S as Into<usize>>::into(slot)].take() {
            self.free.push(slot).unwrap();
            Some(item)
        } else {
            None
        }
    }

    pub fn get(&self, slot: S) -> Option<&T> {
        self.slots[<S as Into<usize>>::into(slot)].as_ref()
    }

    pub fn get_mut(&mut self, slot: S) -> Option<&mut T> {
        self.slots[<S as Into<usize>>::into(slot)].as_mut()
    }
}

pub struct MappedSlots<
    T,
    S: PrimInt + Unsigned + Scalar + Into<usize> + core::iter::Step + Debug,
    const W: usize,
    const H: usize,
    const N: usize,
> {
    slots: Slots<T, S, N>,
    map: [[S; W]; H],
}

impl<
    T,
    S: PrimInt + Unsigned + Scalar + Into<usize> + core::iter::Step + Debug,
    const W: usize,
    const H: usize,
    const N: usize,
> MappedSlots<T, S, W, H, N>
{
    pub fn new() -> Self {
        Self {
            slots: Slots::new(),
            map: [[S::max_value(); W]; H],
        }
    }
    
    pub unsafe fn init(&mut self) {
        self.slots.init();
        self.map.iter_mut().for_each(|it| it.fill_with(S::max_value))
    }

    pub fn get_slot_at<P: PrimInt + Unsigned + Scalar + Into<usize>>(
        &self,
        pos: TVec2<P>,
    ) -> Result<Option<S>, MappedSlotsOutOfBoundsError> {
        let slot_ref = self
            .map
            .get(pos.y.into())
            .ok_or(MappedSlotsOutOfBoundsError)?
            .get(pos.x.into())
            .ok_or(MappedSlotsOutOfBoundsError)?;
        if *slot_ref != S::max_value() {
            Ok(Some(*slot_ref))
        } else {
            Ok(None)
        }
    }
    
    pub unsafe fn set_slot_at<P: PrimInt + Unsigned + Scalar + Into<usize>>(
        &mut self,
        pos: TVec2<P>,
        slot: Option<S>,
    ) -> Result<(), MappedSlotsOutOfBoundsError> {
        let slot = slot.unwrap_or(S::max_value());
        let slot_ref = self.get_raw_slot_mut(pos).ok_or(MappedSlotsOutOfBoundsError)?;
        *slot_ref = slot;
        Ok(())
    }

    fn get_raw_slot_mut<P: PrimInt + Unsigned + Scalar + Into<usize>>(&mut self, pos: TVec2<P>) -> Option<&mut S> {
        Some(self.map.get_mut(pos.y.into())?.get_mut(pos.x.into())?)
    }

    pub fn push_at<P: PrimInt + Unsigned + Scalar + Into<usize>>(
        &mut self,
        pos: TVec2<P>,
        item: T,
    ) -> Result<S, MappedSlotsPushError> {
        let slot_ref = self.get_raw_slot_mut(pos).ok_or(MappedSlotsPushError::PosOutOfBounds)? as *mut S;
        if unsafe { *slot_ref } == S::max_value() {
            let slot = self.slots.push(item).map_err(|_| MappedSlotsPushError::Full)?;
            unsafe {
                *slot_ref = slot;
            }
            Ok(slot)
        } else {
            Err(MappedSlotsPushError::AlreadyOccupied)
        }
    }

    pub fn pop_at<P: PrimInt + Unsigned + Scalar + Into<usize>>(
        &mut self,
        pos: TVec2<P>,
    ) -> Result<T, MappedSlotsPopAtError> {
        let slot_ref = self.get_raw_slot_mut(pos).ok_or(MappedSlotsPopAtError::PosOutOfBounds)?;
        let slot = mem::replace(slot_ref, S::max_value());
        if slot != S::max_value() {
            let item = self.slots.pop(slot).unwrap();
            Ok(item)
        } else {
            Err(MappedSlotsPopAtError::NotOccupied)
        }
    }

    pub fn get(&self, slot: S) -> Option<&T> {
        self.slots.get(slot)
    }

    pub fn get_mut(&mut self, slot: S) -> Option<&mut T> {
        self.slots.get_mut(slot)
    }

    pub fn get_at<P: PrimInt + Unsigned + Scalar + Into<usize>>(
        &self,
        pos: TVec2<P>,
    ) -> Result<Option<SlotItem<&T, S>>, MappedSlotsOutOfBoundsError> {
        let slot = self.get_slot_at(pos)?;
        Ok(slot.map(|slot| SlotItem {
            slot,
            item: self.get(slot).unwrap(),
        }))
    }

    pub fn get_at_mut<P: PrimInt + Unsigned + Scalar + Into<usize>>(
        &mut self,
        pos: TVec2<P>,
    ) -> Result<Option<SlotItem<&mut T, S>>, MappedSlotsOutOfBoundsError> {
        let slot = self.get_slot_at(pos)?;
        Ok(slot.map(|slot| SlotItem {
            slot,
            item: self.get_mut(slot).unwrap(),
        }))
    }
}

pub struct SlotItem<T, S> {
    pub item: T,
    pub slot: S,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MappedSlotsPushError {
    PosOutOfBounds,
    AlreadyOccupied,
    Full,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MappedSlotsPopAtError {
    PosOutOfBounds,
    NotOccupied,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MappedSlotsOutOfBoundsError;
