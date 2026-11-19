#[derive(Clone, Debug, Default)]
pub struct GpuSnapshot {
    pub utilization_percent: u32,
    pub temperature_c: u32,
    pub clock_mhz: u32,
    pub max_clock_mhz: u32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
}

#[derive(Clone, Debug, Default)]
pub struct BatterySnapshot {
    pub ac_online: bool,
    pub charging: bool,
    pub percent: Option<u8>,
    pub remaining_mwh: u32,
    pub max_capacity_mwh: u32,
    pub rate_mw: u32,
    pub seconds_remaining: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct SystemSnapshot {
    pub cpu_load_percent: f32,
    pub cpu_clock_mhz: u32,
    pub cpu_max_clock_mhz: u32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub gpu: Option<GpuSnapshot>,
    pub battery: Option<BatterySnapshot>,
}

impl SystemSnapshot {
    pub fn memory_percent(&self) -> u32 {
        percent(self.memory_used_bytes, self.memory_total_bytes)
    }

    pub fn disk_percent(&self) -> u32 {
        percent(self.disk_used_bytes, self.disk_total_bytes)
    }
}

fn percent(used: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    ((used as f64 / total as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u32
}

#[cfg(windows)]
pub(crate) use windows_impl::CpuPerformanceSampler;
#[cfg(windows)]
pub use windows_impl::{SystemSampler, cpu_frequency_smoke};

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CpuPerformanceReading {
    pub load_percent: Option<f32>,
    pub clock_mhz: Option<u32>,
    pub nominal_mhz: Option<u32>,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy)]
struct CpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

#[cfg(any(windows, test))]
fn cpu_load_percent(previous: CpuTimes, current: CpuTimes) -> Option<f32> {
    let kernel = current.kernel.checked_sub(previous.kernel)?;
    let user = current.user.checked_sub(previous.user)?;
    let idle = current.idle.checked_sub(previous.idle)?;
    let total = kernel.checked_add(user)?;
    if total == 0 || idle > total {
        return None;
    }
    Some(((total - idle) as f64 / total as f64 * 100.0) as f32)
}

// PDH_FMT_NOCAP100 from the Windows SDK's Pdh.h. windows-sys does not
// expose this flag; processor performance legitimately exceeds 100% in boost.
#[cfg(any(windows, test))]
const CPU_PERFORMANCE_FORMAT: u32 = 0x0000_0200 | 0x0000_8000;

#[cfg(any(windows, test))]
fn performance_clock_mhz(performance: Option<f64>, nominal_mhz: u32) -> u32 {
    let Some(performance) = performance.filter(|value| value.is_finite() && *value > 0.0) else {
        return 0;
    };
    (performance / 100.0 * f64::from(nominal_mhz)).round() as u32
}

#[cfg(test)]
mod frequency_tests {
    use super::*;

    #[test]
    fn performance_format_preserves_boost_values() {
        assert_eq!(CPU_PERFORMANCE_FORMAT, 0x8200);
        assert_eq!(performance_clock_mhz(Some(142.0), 3600), 5112);
        assert_eq!(performance_clock_mhz(Some(100.0), 3600), 3600);
        assert_eq!(performance_clock_mhz(Some(12.5), 3600), 450);
    }

    #[test]
    fn missing_performance_does_not_become_a_nominal_clock() {
        for value in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            assert_eq!(performance_clock_mhz(value, 3600), 0);
        }
        assert_eq!(performance_clock_mhz(Some(142.0), 0), 0);
    }

    #[test]
    fn cpu_load_distinguishes_idle_from_unavailable_intervals() {
        let initial = CpuTimes {
            idle: 10,
            kernel: 20,
            user: 30,
        };
        assert_eq!(
            cpu_load_percent(
                initial,
                CpuTimes {
                    idle: 30,
                    kernel: 40,
                    user: 50
                }
            ),
            Some(50.0)
        );
        assert_eq!(
            cpu_load_percent(
                initial,
                CpuTimes {
                    idle: 30,
                    kernel: 40,
                    user: 30
                }
            ),
            Some(0.0)
        );
        assert_eq!(cpu_load_percent(initial, initial), None);
        assert_eq!(
            cpu_load_percent(
                initial,
                CpuTimes {
                    idle: 9,
                    kernel: 40,
                    user: 50
                }
            ),
            None
        );
        assert_eq!(
            cpu_load_percent(
                initial,
                CpuTimes {
                    idle: 90,
                    kernel: 40,
                    user: 50
                }
            ),
            None
        );
    }
}

#[cfg(not(windows))]
pub use demo_impl::SystemSampler;

#[cfg(windows)]
mod windows_impl {
    use super::{BatterySnapshot, CpuPerformanceReading, CpuTimes, GpuSnapshot, SystemSnapshot};
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{FILETIME, FreeLibrary, HMODULE};
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::Performance::{
        PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
        PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
        PdhGetFormattedCounterValue, PdhOpenQueryW,
    };
    use windows_sys::Win32::System::Power::{
        CallNtPowerInformation, GetSystemPowerStatus, ProcessorInformation, SYSTEM_BATTERY_STATE,
        SYSTEM_POWER_STATUS, SystemBatteryState,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::{
        ALL_PROCESSOR_GROUPS, GetActiveProcessorCount, GetSystemTimes,
    };

    /// CPU-only sampling shared by the resident UI and headless boot service.
    /// Opening this sampler does not initialize NVML, a window, or disk queries.
    pub(crate) struct CpuPerformanceSampler {
        previous_cpu: Option<CpuTimes>,
        cpu_frequency: Option<CpuFrequencySampler>,
        next_frequency_retry: Instant,
    }

    impl CpuPerformanceSampler {
        pub(crate) fn new() -> Self {
            Self {
                previous_cpu: read_cpu_times(),
                cpu_frequency: CpuFrequencySampler::open(),
                next_frequency_retry: Instant::now() + Duration::from_secs(5),
            }
        }

        pub(crate) fn sample(&mut self) -> CpuPerformanceReading {
            let cpu = read_cpu_times();
            let load_percent = self
                .previous_cpu
                .zip(cpu)
                .and_then(|(previous, current)| super::cpu_load_percent(previous, current));
            self.previous_cpu = cpu;
            if self.cpu_frequency.is_none() && Instant::now() >= self.next_frequency_retry {
                self.cpu_frequency = CpuFrequencySampler::open();
                self.next_frequency_retry = Instant::now() + Duration::from_secs(5);
            }
            let (cpu_clock_mhz, cpu_max_clock_mhz) = self
                .cpu_frequency
                .as_mut()
                .map(CpuFrequencySampler::sample)
                .unwrap_or((0, 0));

            CpuPerformanceReading {
                load_percent,
                clock_mhz: (cpu_clock_mhz > 0).then_some(cpu_clock_mhz),
                nominal_mhz: (cpu_max_clock_mhz > 0).then_some(cpu_max_clock_mhz),
            }
        }
    }

    pub struct SystemSampler {
        cpu: CpuPerformanceSampler,
        nvml: Option<Nvml>,
    }

    impl SystemSampler {
        pub fn new() -> Self {
            Self {
                cpu: CpuPerformanceSampler::new(),
                nvml: Nvml::open(),
            }
        }

        pub fn sample(&mut self) -> SystemSnapshot {
            let cpu = self.cpu.sample();
            let (memory_used_bytes, memory_total_bytes) = read_memory();
            let (disk_used_bytes, disk_total_bytes) = read_disk();

            SystemSnapshot {
                cpu_load_percent: cpu.load_percent.unwrap_or_default(),
                cpu_clock_mhz: cpu.clock_mhz.unwrap_or_default(),
                cpu_max_clock_mhz: cpu.nominal_mhz.unwrap_or_default(),
                memory_used_bytes,
                memory_total_bytes,
                disk_used_bytes,
                disk_total_bytes,
                gpu: self.nvml.as_ref().and_then(Nvml::sample),
                battery: read_battery(),
            }
        }
    }

    fn read_cpu_times() -> Option<CpuTimes> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: all three FILETIME outputs refer to live local storage and
        // GetSystemTimes writes exactly one initialized value to each.
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        (ok != 0).then(|| CpuTimes {
            idle: filetime(idle),
            kernel: filetime(kernel),
            user: filetime(user),
        })
    }

    fn filetime(value: FILETIME) -> u64 {
        (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
    }

    fn read_memory() -> (u64, u64) {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: dwLength is initialized to the exact structure size and the
        // pointer refers to writable local storage for the duration of the call.
        if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
            return (0, 0);
        }
        (
            status.ullTotalPhys.saturating_sub(status.ullAvailPhys),
            status.ullTotalPhys,
        )
    }

    fn read_disk() -> (u64, u64) {
        let mut free = 0_u64;
        let mut total = 0_u64;
        let mut total_free = 0_u64;
        let path: Vec<u16> = std::ffi::OsStr::new(r"C:\")
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: `path` is NUL-terminated and all three output pointers refer
        // to live u64 locals for the duration of GetDiskFreeSpaceExW.
        let ok =
            unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, &mut total_free) };
        if ok == 0 {
            (0, 0)
        } else {
            (total.saturating_sub(total_free), total)
        }
    }

    fn read_battery() -> Option<BatterySnapshot> {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: `status` is a live SYSTEM_POWER_STATUS value that the Windows
        // API initializes; no pointer escapes this function.
        if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
            return None;
        }
        if status.BatteryFlag == 255 || status.BatteryFlag & 128 != 0 {
            return None;
        }

        let mut state = SYSTEM_BATTERY_STATE::default();
        let size = std::mem::size_of::<SYSTEM_BATTERY_STATE>() as u32;
        // SAFETY: `state` is a live SYSTEM_BATTERY_STATE buffer whose size is
        // passed exactly to CallNtPowerInformation for SystemBatteryState.
        let state_status = unsafe {
            CallNtPowerInformation(
                SystemBatteryState,
                std::ptr::null(),
                0,
                (&mut state as *mut SYSTEM_BATTERY_STATE).cast(),
                size,
            )
        };
        let state_valid = state_status >= 0 && state.BatteryPresent;
        let seconds_remaining = if status.BatteryLifeTime != u32::MAX && status.BatteryLifeTime != 0
        {
            Some(status.BatteryLifeTime)
        } else if state_valid && state.EstimatedTime != 0 && state.EstimatedTime != u32::MAX {
            Some(state.EstimatedTime)
        } else {
            None
        };

        Some(BatterySnapshot {
            ac_online: status.ACLineStatus == 1 || (state_valid && state.AcOnLine),
            charging: status.BatteryFlag & 8 != 0 || (state_valid && state.Charging),
            percent: (status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent),
            remaining_mwh: if state_valid {
                state.RemainingCapacity
            } else {
                0
            },
            max_capacity_mwh: if state_valid { state.MaxCapacity } else { 0 },
            rate_mw: if state_valid { state.Rate } else { 0 },
            seconds_remaining,
        })
    }

    struct PdhQuery(PDH_HQUERY);

    impl PdhQuery {
        fn open() -> Option<Self> {
            let mut query = std::ptr::null_mut();
            // SAFETY: `query` is a live output pointer; on success the returned
            // PDH handle is owned by the PdhQuery wrapper.
            (unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) } == 0).then_some(Self(query))
        }

        fn add_english_counter(&self, path: &str) -> Option<PDH_HCOUNTER> {
            let path = path
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let mut counter = std::ptr::null_mut();
            // SAFETY: the query handle is valid for `self`, `path` is
            // NUL-terminated and alive for the call, and `counter` is writable.
            let status = unsafe { PdhAddEnglishCounterW(self.0, path.as_ptr(), 0, &mut counter) };
            (status == 0).then_some(counter)
        }

        fn collect(&self) -> bool {
            // SAFETY: `self.0` is the live PDH query handle owned by this value.
            unsafe { PdhCollectQueryData(self.0) == 0 }
        }
    }

    impl Drop for PdhQuery {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: this wrapper owns the non-null PDH query handle and
                // closes it exactly once in Drop.
                unsafe { PdhCloseQuery(self.0) };
            }
        }
    }

    struct CpuFrequencySampler {
        query: PdhQuery,
        performance: PDH_HCOUNTER,
        nominal_mhz: u32,
    }

    impl CpuFrequencySampler {
        fn open() -> Option<Self> {
            let nominal_mhz = read_nominal_clock_mhz();
            if nominal_mhz == 0 {
                return None;
            }
            let query = PdhQuery::open()?;
            let performance = query
                .add_english_counter(r"\Processor Information(_Total)\% Processor Performance")?;
            query.collect();
            Some(Self {
                query,
                performance,
                nominal_mhz,
            })
        }

        fn sample(&mut self) -> (u32, u32) {
            if !self.query.collect() {
                return (0, self.nominal_mhz);
            }
            let performance = formatted_value(self.performance, super::CPU_PERFORMANCE_FORMAT);
            let current = super::performance_clock_mhz(performance, self.nominal_mhz);
            (current, self.nominal_mhz)
        }
    }

    pub fn cpu_frequency_smoke() -> i32 {
        let Some(mut sampler) = CpuFrequencySampler::open() else {
            eprintln!("CPU performance counter is unavailable");
            return 1;
        };
        let frequency = sampler
            .query
            .add_english_counter(r"\Processor Information(_Total)\Processor Frequency");
        let mut valid = 0;
        for index in 0..12 {
            std::thread::sleep(Duration::from_millis(250));
            let (current, nominal) = sampler.sample();
            // Both formatted reads use the query's same collected sample.
            let default_format = formatted_value(sampler.performance, PDH_FMT_DOUBLE);
            let uncapped = formatted_value(sampler.performance, super::CPU_PERFORMANCE_FORMAT);
            let reported = frequency.and_then(|counter| formatted_value(counter, PDH_FMT_DOUBLE));
            let expected = super::performance_clock_mhz(uncapped, nominal);
            println!(
                "cpu_frequency sample={index} nominal_mhz={nominal} reported_mhz={reported:?} default_format_percent={default_format:?} performance_percent={uncapped:?} current_mhz={current} expected_mhz={expected}"
            );
            if current != expected {
                return 1;
            }
            valid += usize::from(current > 0);
        }
        println!("cpu_frequency valid_samples={valid}/12");
        i32::from(valid == 0)
    }

    fn formatted_value(counter: PDH_HCOUNTER, format: u32) -> Option<f64> {
        // SAFETY: `counter` is a live PDH counter returned by this query; the
        // format and output pointers match PdhGetFormattedCounterValue's ABI.
        let value = unsafe {
            let mut value = PDH_FMT_COUNTERVALUE::default();
            let mut counter_type = 0_u32;
            let status =
                PdhGetFormattedCounterValue(counter, format, &mut counter_type, &mut value);
            if status != 0
                || (value.CStatus != PDH_CSTATUS_VALID_DATA
                    && value.CStatus != PDH_CSTATUS_NEW_DATA)
            {
                None
            } else {
                Some(value.Anonymous.doubleValue)
            }
        };
        value.filter(|value| value.is_finite())
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct ProcessorPowerInformation {
        number: u32,
        max_mhz: u32,
        current_mhz: u32,
        mhz_limit: u32,
        max_idle_state: u32,
        current_idle_state: u32,
    }

    fn read_nominal_clock_mhz() -> u32 {
        // SAFETY: GetActiveProcessorCount takes only a scalar group selector
        // and has no pointer or lifetime preconditions.
        let count = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }.max(1) as usize;
        let mut info = vec![ProcessorPowerInformation::default(); count];
        let bytes = std::mem::size_of_val(info.as_slice()) as u32;
        // SAFETY: `info` contains `count` repr(C) entries and `bytes` is exactly
        // their allocated size; ProcessorInformation initializes that buffer.
        let status = unsafe {
            CallNtPowerInformation(
                ProcessorInformation,
                std::ptr::null(),
                0,
                info.as_mut_ptr().cast(),
                bytes,
            )
        };
        if status < 0 {
            return 0;
        }
        info.iter().map(|item| item.max_mhz).max().unwrap_or(0)
    }

    #[repr(C)]
    #[derive(Default)]
    struct NvmlUtilization {
        gpu: u32,
        memory: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct NvmlMemory {
        total: u64,
        free: u64,
        used: u64,
    }

    type Device = *mut c_void;
    type NvmlUtilizationFn = unsafe extern "system" fn(Device, *mut NvmlUtilization) -> i32;
    type NvmlTemperatureFn = unsafe extern "system" fn(Device, i32, *mut u32) -> i32;
    type NvmlClockFn = unsafe extern "system" fn(Device, i32, *mut u32) -> i32;
    type NvmlMemoryFn = unsafe extern "system" fn(Device, *mut NvmlMemory) -> i32;
    type NvmlInitFn = unsafe extern "system" fn() -> i32;
    type NvmlCountFn = unsafe extern "system" fn(*mut u32) -> i32;
    type NvmlHandleFn = unsafe extern "system" fn(u32, *mut Device) -> i32;

    struct ModuleHandle(HMODULE);

    impl ModuleHandle {
        fn raw(&self) -> HMODULE {
            self.0
        }
    }

    impl Drop for ModuleHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: this wrapper owns the non-null module handle returned
                // by LoadLibraryW and releases it exactly once in Drop.
                unsafe { FreeLibrary(self.0) };
            }
        }
    }

    struct Nvml {
        _module: ModuleHandle,
        device: Device,
        get_utilization: NvmlUtilizationFn,
        get_temperature: NvmlTemperatureFn,
        get_clock: NvmlClockFn,
        get_memory: NvmlMemoryFn,
    }

    impl Nvml {
        fn open() -> Option<Self> {
            let module = load_nvml()?;
            // SAFETY: `module` remains loaded for the lifetime of the returned
            // Nvml value. Every symbol is requested by its exact NVML name and
            // its function pointer type matches the documented C ABI.
            unsafe {
                let init: Option<NvmlInitFn> = symbol(module.raw(), b"nvmlInit_v2\0");
                let get_count: Option<NvmlCountFn> =
                    symbol(module.raw(), b"nvmlDeviceGetCount_v2\0");
                let get_handle: Option<NvmlHandleFn> =
                    symbol(module.raw(), b"nvmlDeviceGetHandleByIndex_v2\0");
                let get_utilization = symbol(module.raw(), b"nvmlDeviceGetUtilizationRates\0")?;
                let get_temperature = symbol(module.raw(), b"nvmlDeviceGetTemperature\0")?;
                let get_clock = symbol(module.raw(), b"nvmlDeviceGetClockInfo\0")?;
                let get_memory = symbol(module.raw(), b"nvmlDeviceGetMemoryInfo\0")?;

                if init?() != 0 {
                    return None;
                }
                let mut count = 0_u32;
                if get_count?(&mut count) != 0 || count == 0 {
                    return None;
                }
                let mut device = std::ptr::null_mut();
                if get_handle?(0, &mut device) != 0 || device.is_null() {
                    return None;
                }
                Some(Self {
                    _module: module,
                    device,
                    get_utilization,
                    get_temperature,
                    get_clock,
                    get_memory,
                })
            }
        }

        fn sample(&self) -> Option<GpuSnapshot> {
            let mut utilization = NvmlUtilization::default();
            let mut temperature = 0_u32;
            let mut clock = 0_u32;
            let mut max_clock = 0_u32;
            let mut memory = NvmlMemory::default();
            // SAFETY: the NVML device handle was validated during initialization;
            // all output pointers refer to live repr(C) structures with matching
            // ABI layouts for these documented NVML entry points.
            unsafe {
                if (self.get_utilization)(self.device, &mut utilization) != 0
                    || (self.get_temperature)(self.device, 0, &mut temperature) != 0
                    || (self.get_clock)(self.device, 0, &mut clock) != 0
                    || (self.get_clock)(self.device, 1, &mut max_clock) != 0
                    || (self.get_memory)(self.device, &mut memory) != 0
                {
                    return None;
                }
            }
            Some(GpuSnapshot {
                utilization_percent: utilization.gpu.min(100),
                temperature_c: temperature,
                clock_mhz: clock,
                max_clock_mhz: max_clock,
                memory_used_bytes: memory.used,
                memory_total_bytes: memory.total,
            })
        }
    }

    unsafe fn symbol<T: Copy>(module: HMODULE, name: &[u8]) -> Option<T> {
        // SAFETY: callers pass a live module and NUL-terminated symbol name;
        // GetProcAddress reads both only for the duration of this call.
        let proc = unsafe { GetProcAddress(module, name.as_ptr()) }?;
        // SAFETY: the caller selects `T` to match the named exported function's
        // ABI; transmute_copy copies the function pointer representation.
        Some(unsafe { std::mem::transmute_copy(&proc) })
    }

    fn load_nvml() -> Option<ModuleHandle> {
        let mut candidates = vec![
            "nvml.dll".to_owned(),
            r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll".to_owned(),
            r"C:\Windows\System32\nvml.dll".to_owned(),
        ];
        if let Ok(root) = std::env::var("ProgramFiles") {
            candidates.push(
                Path::new(&root)
                    .join("NVIDIA Corporation")
                    .join("NVSMI")
                    .join("nvml.dll")
                    .display()
                    .to_string(),
            );
        }
        for candidate in candidates {
            let mut wide: Vec<u16> = std::ffi::OsStr::new(&candidate)
                .encode_wide()
                .chain(Some(0))
                .collect();
            // SAFETY: `wide` is NUL-terminated and valid for the duration of
            // LoadLibraryW; the returned handle is owned by ModuleHandle.
            let module = unsafe { LoadLibraryW(wide.as_mut_ptr()) };
            if !module.is_null() {
                return Some(ModuleHandle(module));
            }
        }
        None
    }
}

#[cfg(not(windows))]
mod demo_impl {
    use super::SystemSnapshot;
    use std::time::Instant;

    pub struct SystemSampler {
        started: Instant,
    }

    impl SystemSampler {
        pub fn new() -> Self {
            Self {
                started: Instant::now(),
            }
        }

        pub fn sample(&mut self) -> SystemSnapshot {
            let t = self.started.elapsed().as_secs_f32();
            SystemSnapshot {
                cpu_load_percent: 18.0 + (t * 0.9).sin() * 7.0,
                cpu_clock_mhz: 4_260,
                cpu_max_clock_mhz: 5_200,
                memory_used_bytes: 12_884_901_888,
                memory_total_bytes: 34_359_738_368,
                disk_used_bytes: 614_000_000_000,
                disk_total_bytes: 1_024_000_000_000,
                gpu: Some(super::GpuSnapshot {
                    utilization_percent: 34,
                    temperature_c: 55,
                    clock_mhz: 2_100,
                    max_clock_mhz: 2_595,
                    memory_used_bytes: 2_400_000_000,
                    memory_total_bytes: 8_589_934_592,
                }),
                battery: Some(super::BatterySnapshot {
                    ac_online: true,
                    charging: false,
                    percent: Some(78),
                    remaining_mwh: 62_400,
                    max_capacity_mwh: 80_000,
                    rate_mw: 18_500,
                    seconds_remaining: Some(12_000),
                }),
            }
        }
    }
}
