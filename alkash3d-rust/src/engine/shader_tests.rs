// Компилирует каждый шейдер из engine/shaders/ тем же путём, что и движок
// (`ShaderBlob::compile`), но без GPU — D3DCompile работает на CPU, так что
// тест безопасно запускать без запуска самого движка.
use crate::shader::ShaderBlob;

fn compile(name: &str, source: &str, target: &str) {
    ShaderBlob::compile(source, target, "main")
        .unwrap_or_else(|e| panic!("{} не скомпилировался: {:?}", name, e));
}

#[test]
fn all_shaders_compile() {
    compile("main_vs", include_str!("shaders/main_vs.hlsl"), "vs_5_0");
    compile("main_ps", include_str!("shaders/main_ps.hlsl"), "ps_5_0");
    compile("shadow_vs", include_str!("shaders/shadow_vs.hlsl"), "vs_5_0");
    compile("occluder_vs", include_str!("shaders/occluder_vs.hlsl"), "vs_5_0");
    compile("fullscreen_vs", include_str!("shaders/fullscreen_vs.hlsl"), "vs_5_0");
    compile("tonemap_ps", include_str!("shaders/tonemap_ps.hlsl"), "ps_5_0");
    compile("bloom_extract_ps", include_str!("shaders/bloom_extract_ps.hlsl"), "ps_5_0");
    compile("bloom_blur_ps", include_str!("shaders/bloom_blur_ps.hlsl"), "ps_5_0");

    // SSAO и volumetric собираются в двух вариантах — с MSAA и без (см.
    // compile_ssao_shaders / compile_volumetric_shaders).
    for msaa_define in ["", "#define MSAA 1\n#line 1\n"] {
        let ssao = format!("{}{}", msaa_define, include_str!("shaders/ssao_ps.hlsl"));
        let volumetric = format!("{}{}", msaa_define, include_str!("shaders/volumetric_ps.hlsl"));
        compile("ssao_ps", &ssao, "ps_5_0");
        compile("volumetric_ps", &volumetric, "ps_5_0");
    }
}
