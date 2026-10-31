use wasm_bindgen::prelude::*;

/// `microhear_core::Ear` for JavaScript: push worklet samples, get one level per finished frame.
#[wasm_bindgen]
pub struct Ear {
    inner: microhear_core::Ear,
    reports: Vec<microhear_core::FrameReport>,
}

#[wasm_bindgen]
impl Ear {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: microhear_core::Ear::new(),
            reports: Vec::new(),
        }
    }

    /// Levels in dBFS of the frames this batch completed, oldest first. Often empty.
    pub fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        self.reports.clear();
        self.inner.push(samples, &mut self.reports);
        self.reports.iter().map(|r| r.level_db).collect()
    }
}

impl Default for Ear {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub fn sample_rate() -> u32 {
    microhear_core::SAMPLE_RATE
}
