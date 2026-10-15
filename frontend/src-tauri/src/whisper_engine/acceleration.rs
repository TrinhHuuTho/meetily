use crate::audio::GpuType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperCompiledBackend {
    Metal,
    Cuda,
    Vulkan,
    HipBlas,
    Cpu,
}

impl WhisperCompiledBackend {
    pub fn current() -> Self {
        if cfg!(feature = "cuda") {
            Self::Cuda
        } else if cfg!(feature = "vulkan") {
            Self::Vulkan
        } else if cfg!(feature = "hipblas") {
            Self::HipBlas
        } else if cfg!(target_os = "macos") || cfg!(feature = "metal") {
            Self::Metal
        } else {
            Self::Cpu
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metal => "Metal",
            Self::Cuda => "Cuda",
            Self::Vulkan => "Vulkan",
            Self::HipBlas => "HipBlas",
            Self::Cpu => "Cpu",
        }
    }
}

/// A GPU that whisper.cpp can run on
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperGpuDevice {
    /// whisper.cpp `gpu_device` index (counts GPU and integrated GPU devices only)
    pub index: i32,
    pub name: String,
    pub description: String,
    pub integrated: bool,
    pub memory_total_mb: u64,
}

/// GPUs the compiled-in ggml backends can use, in whisper.cpp's `gpu_device` numbering.
/// Enumerated once; device enumeration creates a Vulkan instance on Vulkan builds.
pub fn gpu_devices() -> &'static [WhisperGpuDevice] {
    static DEVICES: std::sync::OnceLock<Vec<WhisperGpuDevice>> = std::sync::OnceLock::new();
    DEVICES.get_or_init(|| {
        use std::ffi::CStr;
        use whisper_rs::whisper_rs_sys as sys;

        let mut devices = Vec::new();
        // SAFETY: the ggml device registry is process-global and lives for the whole
        // program; the returned name/description strings are owned by the backend.
        unsafe {
            for i in 0..sys::ggml_backend_dev_count() {
                let dev = sys::ggml_backend_dev_get(i);
                let dev_type = sys::ggml_backend_dev_type(dev);
                let integrated = dev_type == sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU;
                if dev_type != sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU && !integrated {
                    continue;
                }
                let (mut free, mut total) = (0usize, 0usize);
                sys::ggml_backend_dev_memory(dev, &mut free, &mut total);
                devices.push(WhisperGpuDevice {
                    index: devices.len() as i32,
                    name: CStr::from_ptr(sys::ggml_backend_dev_name(dev)).to_string_lossy().into_owned(),
                    description: CStr::from_ptr(sys::ggml_backend_dev_description(dev)).to_string_lossy().into_owned(),
                    integrated,
                    memory_total_mb: (total / (1024 * 1024)) as u64,
                });
            }
        }
        for device in &devices {
            log::info!(
                "Whisper GPU device {}: {} ({}, {}, {} MB)",
                device.index,
                device.name,
                device.description,
                if device.integrated { "integrated" } else { "discrete" },
                device.memory_total_mb,
            );
        }
        devices
    })
}

/// Prefer a discrete GPU (laptops often list the weaker integrated GPU first)
pub fn preferred_gpu(devices: &[WhisperGpuDevice]) -> Option<&WhisperGpuDevice> {
    devices.iter().find(|device| !device.integrated).or_else(|| devices.first())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhisperContextAcceleration {
    pub compiled_backend: WhisperCompiledBackend,
    pub runtime_detected_gpu: GpuType,
    pub use_gpu: bool,
    pub flash_attn: bool,
    pub gpu_device: i32,
}

impl WhisperContextAcceleration {
    pub fn status_label(self) -> &'static str {
        if !self.use_gpu {
            return "CPU processing only";
        }
        match (self.compiled_backend, self.flash_attn) {
            (WhisperCompiledBackend::Metal, true) => "Metal GPU with Flash Attention (Ultra-Fast)",
            (WhisperCompiledBackend::Metal, false) => "Metal GPU acceleration",
            (WhisperCompiledBackend::Cuda, true) => "CUDA GPU with Flash Attention (Ultra-Fast)",
            (WhisperCompiledBackend::Cuda, false) => "CUDA GPU acceleration",
            (WhisperCompiledBackend::Vulkan, _) => "Vulkan GPU acceleration",
            (WhisperCompiledBackend::HipBlas, _) => "HIP BLAS GPU acceleration",
            (WhisperCompiledBackend::Cpu, _) => "CPU processing only",
        }
    }
}

/// Decide how whisper.cpp should run: on `gpu` when the build has a GPU backend and the
/// user has not asked for CPU only, otherwise on the CPU.
pub fn whisper_context_acceleration_for(
    compiled_backend: WhisperCompiledBackend,
    runtime_detected_gpu: GpuType,
    gpu: Option<&WhisperGpuDevice>,
    cpu_only: bool,
) -> WhisperContextAcceleration {
    let use_gpu = !matches!(compiled_backend, WhisperCompiledBackend::Cpu) && gpu.is_some() && !cpu_only;
    // Flash attention cuts encoder time substantially on every CUDA/Metal GPU, and
    // matters most on low-end cards (e.g. GTX 1650: ~25% faster), so it is not tier-gated.
    let flash_attn = match compiled_backend {
        WhisperCompiledBackend::Metal | WhisperCompiledBackend::Cuda => true,
        WhisperCompiledBackend::Vulkan | WhisperCompiledBackend::HipBlas | WhisperCompiledBackend::Cpu => false,
    };

    WhisperContextAcceleration {
        compiled_backend,
        runtime_detected_gpu,
        use_gpu,
        flash_attn: use_gpu && flash_attn,
        gpu_device: gpu.filter(|_| use_gpu).map_or(0, |device| device.index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(index: i32, integrated: bool) -> WhisperGpuDevice {
        WhisperGpuDevice {
            index,
            name: format!("Vulkan{index}"),
            description: String::new(),
            integrated,
            memory_total_mb: 4096,
        }
    }

    #[test]
    fn prefers_discrete_gpu_over_integrated() {
        let devices = [device(0, true), device(1, false)];
        assert_eq!(preferred_gpu(&devices).map(|d| d.index), Some(1));
        assert_eq!(preferred_gpu(&devices[..1]).map(|d| d.index), Some(0));
        assert_eq!(preferred_gpu(&[]), None);
    }

    #[test]
    fn acceleration_vulkan_uses_selected_gpu_without_flash_attention() {
        let gpu = device(1, false);
        let params = whisper_context_acceleration_for(WhisperCompiledBackend::Vulkan, GpuType::Cuda, Some(&gpu), false);

        assert_eq!(params.compiled_backend, WhisperCompiledBackend::Vulkan);
        assert!(params.use_gpu);
        assert_eq!(params.gpu_device, 1);
        assert!(!params.flash_attn);
    }

    #[test]
    fn acceleration_falls_back_to_cpu_without_a_gpu() {
        let params = whisper_context_acceleration_for(WhisperCompiledBackend::Vulkan, GpuType::None, None, false);

        assert!(!params.use_gpu);
        assert!(!params.flash_attn);
    }

    #[test]
    fn acceleration_cpu_only_preference_disables_gpu() {
        let gpu = device(0, false);
        for backend in [WhisperCompiledBackend::Vulkan, WhisperCompiledBackend::Cuda, WhisperCompiledBackend::Metal] {
            let params = whisper_context_acceleration_for(backend, GpuType::None, Some(&gpu), true);

            assert!(!params.use_gpu);
            assert!(!params.flash_attn);
        }
    }

    #[test]
    fn acceleration_cuda_backend_enables_flash_attention() {
        let gpu = device(0, false);
        let params = whisper_context_acceleration_for(WhisperCompiledBackend::Cuda, GpuType::Cuda, Some(&gpu), false);

        assert!(params.use_gpu);
        assert!(params.flash_attn);
    }

    #[test]
    fn acceleration_cpu_backend_disables_gpu_and_flash_attention() {
        let gpu = device(0, false);
        for runtime_gpu in [GpuType::None, GpuType::Cuda, GpuType::Vulkan] {
            let params = whisper_context_acceleration_for(WhisperCompiledBackend::Cpu, runtime_gpu, Some(&gpu), false);

            assert!(!params.use_gpu);
            assert!(!params.flash_attn);
        }
    }
}
