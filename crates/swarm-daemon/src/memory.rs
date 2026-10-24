use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

/// Trims the process working set, forcing Windows to page out unused memory to keep RAM usage low.
pub fn trim_working_set() {
    unsafe {
        let process = GetCurrentProcess();
        // Passing (SIZE_T)-1 for both min and max tells Windows to empty working set
        SetProcessWorkingSetSize(process, usize::MAX, usize::MAX);
    }
}
