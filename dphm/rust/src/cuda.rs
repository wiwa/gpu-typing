// Generated/maintained with AI; see repository disclaimers.md.
//! Safe workspace ownership around the small C ABI. CUDA still schedules itself.
use crate::{
    unary::{ResolvedAst, SourceNode, Typing},
    Error, TypeId,
};
use std::{
    ffi::{c_char, c_int, c_void, CStr},
    marker::PhantomData,
    rc::Rc,
};

/// Field order and widths mirror `DphmMetrics` in cuda_solver/c_api.h.
#[repr(C)]
#[derive(Default, Clone, Debug)]
pub struct Metrics {
    pub native_ms: f64,
    pub allocation_ms: f64,
    pub peak_device_bytes: u64,
    pub workspace_bytes: u64,
    pub kernel_launches: u64,
    pub primitive_calls: u64,
    pub synchronizations: u64,
    pub macros: u64,
    pub down_batches: u64,
    pub up_batches: u64,
    pub occurs_waves: u64,
    pub upload_ms: f64,
    pub initialize_ms: f64,
    pub down_ms: f64,
    pub u1_ms: f64,
    pub up_ms: f64,
    pub bulk_ms: f64,
    pub occurs_ms: f64,
    pub download_ms: f64,
    pub generated_equations: u64,
    pub merge_ms: f64,
    pub flatten_ms: f64,
    pub class_validation_ms: f64,
    pub host_prepare_ms: f64,
    pub graph_build_ms: f64,
    pub graph_launches: u64,
    pub host_register_ms: f64,
    pub host_unregister_ms: f64,
    pub registered_output_bytes: u64,
}

const _: () = {
    use std::mem::{align_of, offset_of, size_of};
    assert!(size_of::<Metrics>() == 232 && align_of::<Metrics>() == 8);
    assert!(offset_of!(Metrics, peak_device_bytes) == 16);
    assert!(offset_of!(Metrics, macros) == 56 && offset_of!(Metrics, upload_ms) == 88);
    assert!(offset_of!(Metrics, generated_equations) == 152);
    assert!(offset_of!(Metrics, graph_launches) == 200);
    assert!(offset_of!(Metrics, registered_output_bytes) == 224);
};

extern "C" {
    fn dphm_create(out: *mut *mut c_void, error: *mut c_char, capacity: usize) -> c_int;
    fn dphm_destroy(workspace: *mut c_void);
    fn dphm_inference_ms(workspace: *const c_void) -> f64;
    fn dphm_infer_unary_ast(
        workspace: *mut c_void,
        nodes: *const SourceNode,
        count: usize,
        variables: usize,
        arrows: usize,
        checker: c_int,
        instrument: c_int,
        register_output: c_int,
        arrow_types: *mut [TypeId; 2],
        source: *mut [TypeId; 2],
        metrics: *mut Metrics,
        lowering_ms: *mut f64,
        device_ms: *mut f64,
        export_ms: *mut f64,
        error: *mut c_char,
        capacity: usize,
    ) -> c_int;
}

#[derive(Clone, Copy, Debug)]
pub enum Checker {
    A,
    B,
}

pub struct Workspace {
    native: *mut c_void,
    failed: bool,
    /// Page-lock returned buffers for this call; false leaves them pageable.
    pub register_output: bool,
    /// Rust output allocation only; registration is reported by native metrics.
    pub output_allocation_ms: Option<f64>,
    /// Device solving through occurs, excluding initial reset/transfers.
    pub inference_ms: Option<f64>,
    /// Last call's device scan/emission interval.
    pub lowering_ms: Option<f64>,
    /// Source lowering through whole-tree export completion.
    pub source_device_ms: Option<f64>,
    /// Inference end through whole-tree export.
    pub export_ms: Option<f64>,
    // CUDA workspace/device affinity stays on its creating thread.
    _thread: PhantomData<Rc<()>>,
}

impl Workspace {
    pub fn new() -> Result<Self, Error> {
        let mut native = std::ptr::null_mut();
        let mut error = [0; 512];

        // C owns the opaque object; only Drop destroys a successful allocation.
        let code = unsafe { dphm_create(&mut native, error.as_mut_ptr(), error.len()) };
        status(code, &error)?;

        Ok(Self {
            native,
            failed: false,
            register_output: true,
            output_allocation_ms: None,
            inference_ms: None,
            lowering_ms: None,
            source_device_ms: None,
            export_ms: None,
            _thread: PhantomData,
        })
    }

    /// Infer a host-queryable shared type graph for every source node/binder.
    /// Only normalized arrow children and source references are downloaded.
    /// A runtime/API failure disables further calls on this workspace.
    pub fn infer_ast(
        &mut self,
        ast: &ResolvedAst,
        checker: Checker,
        instrument: bool,
    ) -> (Result<Typing, Error>, Metrics) {
        self.inference_ms = None;
        self.lowering_ms = None;
        self.source_device_ms = None;
        self.export_ms = None;
        self.output_allocation_ms = None;

        if self.failed {
            return (
                Err(Error::Cuda(
                    "workspace disabled after native failure".into(),
                )),
                Metrics::default(),
            );
        }

        let allocation = std::time::Instant::now();
        let mut arrows = vec![[0; 2]; ast.arrows];
        let mut source = vec![[0; 2]; ast.node_count()];
        self.output_allocation_ms = Some(allocation.elapsed().as_secs_f64() * 1000.0);
        let mut metrics = Metrics::default();
        let (mut lowering, mut device, mut export) = (-1.0, -1.0, -1.0);
        let mut error = [0; 512];

        // Exclusive initialized buffers and checked syntax outlive all native work.
        // [u32;2] matches both eight-byte C result records, including alignment.
        let code = unsafe {
            dphm_infer_unary_ast(
                self.native,
                ast.nodes.as_ptr(),
                ast.node_count(),
                ast.variables,
                ast.arrows,
                checker as c_int,
                instrument as c_int,
                self.register_output as c_int,
                arrows.as_mut_ptr(),
                source.as_mut_ptr(),
                &mut metrics,
                &mut lowering,
                &mut device,
                &mut export,
                error.as_mut_ptr(),
                error.len(),
            )
        };

        self.failed = code < 0;
        if !self.failed {
            let inference = unsafe { dphm_inference_ms(self.native) };
            self.inference_ms = (inference.is_finite() && inference >= 0.0).then_some(inference);
        }

        self.lowering_ms = (lowering.is_finite() && lowering >= 0.0).then_some(lowering);
        self.source_device_ms = (device.is_finite() && device >= 0.0).then_some(device);
        self.export_ms = (export.is_finite() && export >= 0.0).then_some(export);

        (
            status(code, &error).map(|()| Typing {
                root_node: ast.root as u32,
                first_arrow: (3 + ast.variables) as u32,
                arrows,
                source,
            }),
            metrics,
        )
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        unsafe { dphm_destroy(self.native) };
    }
}

fn status(code: c_int, message: &[c_char; 512]) -> Result<(), Error> {
    match code {
        0 => Ok(()),
        1 => Err(Error::InvalidProblem("CUDA rejected the input")),
        2 => Err(Error::RigidConflict),
        3 => Err(Error::InfiniteType),
        _ => Err(Error::Cuda(
            unsafe { CStr::from_ptr(message.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_workspace_refuses_calls_before_allocation_or_native_work() {
        let mut workspace = Workspace {
            native: std::ptr::null_mut(), // Destroying a null native workspace is valid.
            failed: true,
            register_output: true,
            output_allocation_ms: None,
            inference_ms: None,
            lowering_ms: None,
            source_device_ms: None,
            export_ms: None,
            _thread: PhantomData,
        };
        let source = ResolvedAst::new(&crate::Ast {
            nodes: vec![crate::Node::Base(1)],
            root: 0,
        })
        .unwrap();
        assert!(matches!(
            workspace.infer_ast(&source, Checker::A, false).0,
            Err(Error::Cuda(_))
        ));
        assert_eq!(workspace.output_allocation_ms, None);
        assert_eq!(workspace.inference_ms, None);
    }
}
