use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ShaderManager {
    pub techniques: HashMap<String, ShaderTechnique>,
    pub active_permutation: HashMap<String, u32>,
    pub hot_reload_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ShaderTechnique {
    pub name: String,
    pub permutations: Vec<ShaderPermutation>,
    pub default_permutation: u32,
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct ShaderPermutation {
    pub define_hash: u64,
    pub vs_code: String,
    pub ps_code: String,
    pub pso_cache_key: u64,
    pub rating: u16,
}

impl ShaderManager {
    pub fn new() -> Self {
        let mut techniques = HashMap::new();

        let pbr_permutations = vec![
            ShaderPermutation {
                define_hash: 0,
                vs_code: "PBR_VS_STANDARD".to_string(),
                ps_code: "PBR_PS_STANDARD".to_string(),
                pso_cache_key: 0x1000,
                rating: 100,
            },
            ShaderPermutation {
                define_hash: 1,
                vs_code: "PBR_VS_INSTANCED".to_string(),
                ps_code: "PBR_PS_INSTANCED".to_string(),
                pso_cache_key: 0x1001,
                rating: 90,
            },
        ];

        techniques.insert("PBR_Standard".to_string(), ShaderTechnique {
            name: "PBR Standard".to_string(),
            permutations: pbr_permutations,
            default_permutation: 0,
            flags: 0x01,
        });

        Self {
            techniques,
            active_permutation: HashMap::new(),
            hot_reload_enabled: cfg!(debug_assertions),
        }
    }

    pub fn set_technique(&mut self, name: &str, permutation: u32) {
        self.active_permutation.insert(name.to_string(), permutation);
    }

    pub fn get_active_shader(&self, technique: &str) -> Option<&ShaderPermutation> {
        let tech = self.techniques.get(technique)?;
        let perm_idx = self.active_permutation.get(technique)
            .copied()
            .unwrap_or(tech.default_permutation);
        tech.permutations.get(perm_idx as usize)
    }
}