use core::arch::asm;
use core::sync::atomic::{compiler_fence, AtomicBool, Ordering};

use cortex_m::peripheral::NVIC;
use embassy_nrf::interrupt::Interrupt;

const RESERVED_IRQS: u32 =
    (1 << (Interrupt::RADIO as u8)) | (1 << (Interrupt::RTC0 as u8)) | (1 << (Interrupt::TIMER0 as u8));

static CS_FLAG: AtomicBool = AtomicBool::new(false);
static mut CS_MASK: [u32; 2] = [0; 2];

#[inline]
unsafe fn raw_critical_section<R>(f: impl FnOnce() -> R) -> R {
    // TODO: assert that we're in privileged level
    // Needed because disabling irqs in non-privileged level is a noop, which would break safety.

    let primask: u32;
    asm!("mrs {}, PRIMASK", out(reg) primask);

    asm!("cpsid i");

    // Prevent compiler from reordering operations inside/outside the critical section.
    compiler_fence(Ordering::SeqCst);

    let r = f();

    compiler_fence(Ordering::SeqCst);

    if primask & 1 == 0 {
        asm!("cpsie i");
    }

    r
}

struct CriticalSection;
critical_section::set_impl!(CriticalSection);

unsafe impl critical_section::Impl for CriticalSection {
    unsafe fn acquire() -> bool {
        let nvic = &*NVIC::PTR;
        let nested_cs = CS_FLAG.load(Ordering::SeqCst);

        if !nested_cs {
            raw_critical_section(|| {
                CS_FLAG.store(true, Ordering::Relaxed);

                // Store the state of irqs.
                CS_MASK[0] = nvic.icer[0].read();
                CS_MASK[1] = nvic.icer[1].read();

                // Disable only not-reserved irqs.
                nvic.icer[0].write(!RESERVED_IRQS);
                nvic.icer[1].write(0xFFFF_FFFF);
            });
        }

        compiler_fence(Ordering::SeqCst);

        nested_cs
    }

    unsafe fn release(nested_cs: bool) {
        compiler_fence(Ordering::SeqCst);

        let nvic = &*NVIC::PTR;
        if !nested_cs {
            raw_critical_section(|| {
                CS_FLAG.store(false, Ordering::Relaxed);
                // restore only non-reserved irqs.
                nvic.iser[0].write(CS_MASK[0] & !RESERVED_IRQS);
                nvic.iser[1].write(CS_MASK[1]);
            });
        }
    }
}
