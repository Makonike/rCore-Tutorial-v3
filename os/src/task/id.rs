use super::ProcessControlBlock;
use crate::config::{KERNEL_STACK_SIZE, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT_BASE, USER_STACK_SIZE};
use crate::mm::{MapPermission, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::sync::UPIntrFreeCell;
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use lazy_static::*;

pub struct RecycleAllocator {
    current: usize,
    recycled: Vec<usize>,
}

impl RecycleAllocator {
    pub fn new() -> Self {
        RecycleAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }
    pub fn alloc(&mut self) -> usize {
        if let Some(id) = self.recycled.pop() {
            id
        } else {
            self.current += 1;
            self.current - 1
        }
    }
    pub fn dealloc(&mut self, id: usize) {
        assert!(id < self.current);
        assert!(
            !self.recycled.iter().any(|i| *i == id),
            "id {} has been deallocated!",
            id
        );
        self.recycled.push(id);
    }
}

lazy_static! {
    static ref PID_ALLOCATOR: UPIntrFreeCell<RecycleAllocator> = {
        kprintln!("[KERN] task::id::lazy_static!PID_ALLOCATOR begin");
        unsafe { UPIntrFreeCell::new(RecycleAllocator::new()) }
    };
}

lazy_static! {
    static ref KSTACK_ALLOCATOR: UPIntrFreeCell<RecycleAllocator> ={
        kprintln!("[KERN] task::id::lazy_static!KSTACK_ALLOCATOR begin");
        unsafe { UPIntrFreeCell::new(RecycleAllocator::new()) }
    };
}

pub struct PidHandle(pub usize);

pub fn pid_alloc() -> PidHandle {
    PidHandle(PID_ALLOCATOR.exclusive_access().alloc())
}

impl Drop for PidHandle {
    fn drop(&mut self) {
        kprintln!("[KERN] task::id::Drop<PidHandle>::drop begin");
        PID_ALLOCATOR.exclusive_access().dealloc(self.0);
        kprintln!("[KERN] task::id::Drop<PidHandle>::drop end");
    }
}

/// Return (bottom, top) of a kernel stack in kernel space.
pub fn kernel_stack_position(kstack_id: usize) -> (usize, usize) {
    kprintln!("[KERN] task::id::kernel_stack_position() begin");
    let top = TRAMPOLINE - kstack_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    kprintln!("[KERN] task::id::kernel_stack_position() end");
    (bottom, top)
}

pub struct KernelStack(pub usize);

pub fn kstack_alloc() -> KernelStack {
    kprintln!("[KERN] task::id::kstack_alloc() begin");
    let kstack_id = KSTACK_ALLOCATOR.exclusive_access().alloc();
    let (kstack_bottom, kstack_top) = kernel_stack_position(kstack_id);
    KERNEL_SPACE.exclusive_access().insert_framed_area(
        kstack_bottom.into(),
        kstack_top.into(),
        MapPermission::R | MapPermission::W,
    );
    kprintln!("[KERN] task::id::kstack_alloc() end");
    KernelStack(kstack_id)
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        kprintln!("[KERN] task::id::Drop<KernelStack>::drop begin");
        let (kernel_stack_bottom, _) = kernel_stack_position(self.0);
        let kernel_stack_bottom_va: VirtAddr = kernel_stack_bottom.into();
        KERNEL_SPACE
            .exclusive_access()
            .remove_area_with_start_vpn(kernel_stack_bottom_va.into());
        kprintln!("[KERN] task::id::Drop<KernelStack>::drop end");    
    }
}

impl KernelStack {
    #[allow(unused)]
    pub fn push_on_top<T>(&self, value: T) -> *mut T
    where
        T: Sized,
    {
        let kernel_stack_top = self.get_top();
        let ptr_mut = (kernel_stack_top - core::mem::size_of::<T>()) as *mut T;
        unsafe {
            *ptr_mut = value;
        }
        ptr_mut
    }
    pub fn get_top(&self) -> usize {
        let (_, kernel_stack_top) = kernel_stack_position(self.0);
        kernel_stack_top
    }
}

pub struct TaskUserRes {
    pub tid: usize,
    pub ustack_base: usize,
    pub process: Weak<ProcessControlBlock>,
}

fn trap_cx_bottom_from_tid(tid: usize) -> usize {
    TRAP_CONTEXT_BASE - tid * PAGE_SIZE
}

fn ustack_bottom_from_tid(ustack_base: usize, tid: usize) -> usize {
    ustack_base + tid * (PAGE_SIZE + USER_STACK_SIZE)
}

impl TaskUserRes {
    pub fn new(
        process: Arc<ProcessControlBlock>,
        ustack_base: usize,
        alloc_user_res: bool,
    ) -> Self {
        kprintln!("[KERN] task::id::TaskUserRes::new() begin");
        let tid = process.inner_exclusive_access().alloc_tid();
        let task_user_res = Self {
            tid,
            ustack_base,
            process: Arc::downgrade(&process),
        };
        if alloc_user_res {
            task_user_res.alloc_user_res();
        }
        kprintln!("[KERN] task::id::TaskUserRes::new() end");
        task_user_res
    }

    pub fn alloc_user_res(&self) {
        kprintln!("[KERN] task::id::TaskUserRes::alloc_user_res() begin");
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner_exclusive_access();
        // alloc user stack
        kprintln!("[KERN] task::id::TaskUserRes::alloc_user_res(): alloc user stack for TCB");
        let ustack_bottom = ustack_bottom_from_tid(self.ustack_base, self.tid);
        let ustack_top = ustack_bottom + USER_STACK_SIZE;
        process_inner.memory_set.insert_framed_area(
            ustack_bottom.into(),
            ustack_top.into(),
            MapPermission::R | MapPermission::W | MapPermission::U,
        );
        // alloc trap_cx
        kprintln!("[KERN] task::id::TaskUserRes::alloc_user_res(): alloc trap_cx for TCB");
        let trap_cx_bottom = trap_cx_bottom_from_tid(self.tid);
        let trap_cx_top = trap_cx_bottom + PAGE_SIZE;
        process_inner.memory_set.insert_framed_area(
            trap_cx_bottom.into(),
            trap_cx_top.into(),
            MapPermission::R | MapPermission::W,
        );
        kprintln!("[KERN] task::id::TaskUserRes::alloc_user_res() end");
    }

    fn dealloc_user_res(&self) {
        kprintln!("[KERN] task::id::TaskUserRes::dealloc_user_res() begin");
        // dealloc tid
        kprintln!("[KERN] task::id::TaskUserRes::dealloc_user_res():  dealloc tid");
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner_exclusive_access();
        // dealloc ustack manually
        kprintln!("[KERN] task::id::TaskUserRes::dealloc_user_res():  dealloc ustack manually");
        let ustack_bottom_va: VirtAddr = ustack_bottom_from_tid(self.ustack_base, self.tid).into();
        process_inner
            .memory_set
            .remove_area_with_start_vpn(ustack_bottom_va.into());
        // dealloc trap_cx manually
        kprintln!("[KERN] task::id::TaskUserRes::dealloc_user_res():  dealloc trap_cx  manually");
        let trap_cx_bottom_va: VirtAddr = trap_cx_bottom_from_tid(self.tid).into();
        process_inner
            .memory_set
            .remove_area_with_start_vpn(trap_cx_bottom_va.into());
        
        kprintln!("[KERN] task::id::TaskUserRes::dealloc_user_res() end");    
    }

    #[allow(unused)]
    pub fn alloc_tid(&mut self) {
        self.tid = self
            .process
            .upgrade()
            .unwrap()
            .inner_exclusive_access()
            .alloc_tid();
    }

    pub fn dealloc_tid(&self) {
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner_exclusive_access();
        process_inner.dealloc_tid(self.tid);
    }

    pub fn trap_cx_user_va(&self) -> usize {
        trap_cx_bottom_from_tid(self.tid)
    }

    pub fn trap_cx_ppn(&self) -> PhysPageNum {
        let process = self.process.upgrade().unwrap();
        let process_inner = process.inner_exclusive_access();
        let trap_cx_bottom_va: VirtAddr = trap_cx_bottom_from_tid(self.tid).into();
        process_inner
            .memory_set
            .translate(trap_cx_bottom_va.into())
            .unwrap()
            .ppn()
    }

    pub fn ustack_base(&self) -> usize {
        self.ustack_base
    }
    pub fn ustack_top(&self) -> usize {
        ustack_bottom_from_tid(self.ustack_base, self.tid) + USER_STACK_SIZE
    }
}

impl Drop for TaskUserRes {
    fn drop(&mut self) {
        kprintln!("[KERN] task::id::Drop<TaskUserRes>::drop begin");
        self.dealloc_tid();
        self.dealloc_user_res();
        kprintln!("[KERN] task::id::Drop<TaskUserRes>::drop end");
    }
}