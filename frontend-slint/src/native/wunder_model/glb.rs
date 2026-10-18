use serde_json::Value;

#[derive(Clone, Copy, Debug, Default)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
}

#[derive(Clone, Copy, Debug)]
struct Material {
    base_color: [f32; 3],
    roughness: f32,
    metallic: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: [0.55, 0.58, 0.62],
            roughness: 0.5,
            metallic: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[u32; 3]>,
}

fn read_component(data: &[u8], offset: usize, component: u32) -> f32 {
    match component {
        5126 => f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()),
        5121 => data[offset] as f32,
        5123 => u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap()) as f32,
        5125 => u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as f32,
        _ => 0.0,
    }
}

fn component_size(component: u32) -> usize {
    match component {
        5121 => 1,
        5123 => 2,
        5125 | 5126 => 4,
        _ => 4,
    }
}

fn component_count(kind: &str) -> usize {
    match kind {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        _ => 1,
    }
}

fn accessor_values(json: &Value, bin: &[u8], index: usize) -> Vec<Vec<f32>> {
    let accessor = &json["accessors"][index];
    let view = &json["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let component = accessor["componentType"].as_u64().unwrap() as u32;
    let count = accessor["count"].as_u64().unwrap() as usize;
    let components = component_count(accessor["type"].as_str().unwrap_or("SCALAR"));
    let size = component_size(component);
    let stride = view["byteStride"]
        .as_u64()
        .map(|v| v as usize)
        .unwrap_or(size * components);
    let base = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
    let normalized = accessor["normalized"].as_bool().unwrap_or(false);
    (0..count)
        .map(|row| {
            (0..components)
                .map(|column| {
                    let value = read_component(bin, base + row * stride + column * size, component);
                    if !normalized {
                        return value;
                    }
                    match component {
                        5121 => value / 255.0,
                        5123 => value / 65535.0,
                        5125 => value / 4294967295.0,
                        _ => value,
                    }
                })
                .collect()
        })
        .collect()
}

fn mat_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for row in 0..4 {
        for col in 0..4 {
            out[row][col] = (0..4).map(|i| a[row][i] * b[i][col]).sum();
        }
    }
    out
}

fn transform(m: [[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
    ]
}

fn normal_transform(m: [[f32; 4]; 4], n: [f32; 3]) -> [f32; 3] {
    let n = [
        m[0][0] * n[0] + m[0][1] * n[1] + m[0][2] * n[2],
        m[1][0] * n[0] + m[1][1] * n[1] + m[1][2] * n[2],
        m[2][0] * n[0] + m[2][1] * n[1] + m[2][2] * n[2],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(0.0001);
    [n[0] / length, n[1] / length, n[2] / length]
}

fn node_matrix(node: &Value) -> [[f32; 4]; 4] {
    if let Some(matrix) = node["matrix"].as_array() {
        let mut out = [[0.0; 4]; 4];
        // glTF stores matrices in column-major order; the software rasterizer
        // multiplies row vectors explicitly, so transpose while importing.
        for i in 0..16 {
            out[i % 4][i / 4] = matrix[i].as_f64().unwrap_or(0.0) as f32;
        }
        return out;
    }
    let t = node["translation"]
        .as_array()
        .map(|v| {
            [
                v[0].as_f64().unwrap_or(0.0) as f32,
                v[1].as_f64().unwrap_or(0.0) as f32,
                v[2].as_f64().unwrap_or(0.0) as f32,
            ]
        })
        .unwrap_or([0.0; 3]);
    let s = node["scale"]
        .as_array()
        .map(|v| {
            [
                v[0].as_f64().unwrap_or(1.0) as f32,
                v[1].as_f64().unwrap_or(1.0) as f32,
                v[2].as_f64().unwrap_or(1.0) as f32,
            ]
        })
        .unwrap_or([1.0; 3]);
    let q = node["rotation"]
        .as_array()
        .map(|v| {
            [
                v[0].as_f64().unwrap_or(0.0) as f32,
                v[1].as_f64().unwrap_or(0.0) as f32,
                v[2].as_f64().unwrap_or(0.0) as f32,
                v[3].as_f64().unwrap_or(1.0) as f32,
            ]
        })
        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let [x, y, z, w] = q;
    [
        [
            (1.0 - 2.0 * (y * y + z * z)) * s[0],
            2.0 * (x * y - z * w) * s[1],
            2.0 * (x * z + y * w) * s[2],
            t[0],
        ],
        [
            2.0 * (x * y + z * w) * s[0],
            (1.0 - 2.0 * (x * x + z * z)) * s[1],
            2.0 * (y * z - x * w) * s[2],
            t[1],
        ],
        [
            2.0 * (x * z - y * w) * s[0],
            2.0 * (y * z + x * w) * s[1],
            (1.0 - 2.0 * (x * x + y * y)) * s[2],
            t[2],
        ],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn parse(bytes: &[u8]) -> Result<Scene, String> {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return Err("模型文件格式无效".into());
    }
    let mut offset = 12;
    let mut json_bytes = None;
    let mut bin = None;
    while offset + 8 <= bytes.len() {
        let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        offset += 8;
        if offset + length > bytes.len() {
            return Err("模型文件不完整".into());
        }
        let chunk = &bytes[offset..offset + length];
        if kind == 0x4e4f534a {
            json_bytes = Some(chunk);
        } else if kind == 0x004e4942 {
            bin = Some(chunk);
        }
        offset += length;
    }
    let json: Value = serde_json::from_slice(json_bytes.ok_or("模型缺少场景描述")?)
        .map_err(|_| "模型场景描述无效")?;
    let bin = bin.ok_or("模型缺少几何数据")?;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let materials = json["materials"]
        .as_array()
        .map(|materials| {
            materials
                .iter()
                .map(|material| {
                    let pbr = &material["pbrMetallicRoughness"];
                    let base_color = pbr["baseColorFactor"]
                        .as_array()
                        .map(|factor| {
                            [
                                factor.first().and_then(Value::as_f64).unwrap_or(0.55) as f32,
                                factor.get(1).and_then(Value::as_f64).unwrap_or(0.58) as f32,
                                factor.get(2).and_then(Value::as_f64).unwrap_or(0.62) as f32,
                            ]
                        })
                        .unwrap_or([0.55, 0.58, 0.62]);
                    Material {
                        base_color,
                        roughness: pbr["roughnessFactor"].as_f64().unwrap_or(0.5) as f32,
                        metallic: pbr["metallicFactor"].as_f64().unwrap_or(0.0) as f32,
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    fn visit(
        json: &Value,
        bin: &[u8],
        materials: &[Material],
        node_index: usize,
        parent: [[f32; 4]; 4],
        vertices_out: &mut Vec<Vertex>,
        triangles_out: &mut Vec<[u32; 3]>,
    ) {
        let node = &json["nodes"][node_index];
        let world = mat_mul(parent, node_matrix(node));
        if let Some(mesh_index) = node["mesh"].as_u64() {
            let mesh = &json["meshes"][mesh_index as usize];
            for primitive in mesh["primitives"].as_array().into_iter().flatten() {
                let positions = accessor_values(
                    json,
                    bin,
                    primitive["attributes"]["POSITION"].as_u64().unwrap() as usize,
                );
                let normals = primitive["attributes"]["NORMAL"]
                    .as_u64()
                    .map(|i| accessor_values(json, bin, i as usize));
                let colors = primitive["attributes"]["COLOR_0"]
                    .as_u64()
                    .map(|i| accessor_values(json, bin, i as usize));
                let material = primitive["material"]
                    .as_u64()
                    .and_then(|i| materials.get(i as usize).copied())
                    .unwrap_or_default();
                let indices =
                    accessor_values(json, bin, primitive["indices"].as_u64().unwrap() as usize)
                        .into_iter()
                        .map(|v| v[0] as usize)
                        .collect::<Vec<_>>();
                let vertex_base = vertices_out.len() as u32;
                for (i, position) in positions.iter().enumerate() {
                    let vertex_color = colors
                        .as_ref()
                        .and_then(|v| v.get(i))
                        .map(|v| [v[0], v[1], v[2]])
                        .unwrap_or([1.0, 1.0, 1.0]);
                    vertices_out.push(Vertex {
                        position: transform(world, [position[0], position[1], position[2]]),
                        normal: normal_transform(
                            world,
                            normals
                                .as_ref()
                                .and_then(|v| v.get(i))
                                .map(|v| [v[0], v[1], v[2]])
                                .unwrap_or([0.0, 1.0, 0.0]),
                        ),
                        color: [
                            material.base_color[0] * vertex_color[0],
                            material.base_color[1] * vertex_color[1],
                            material.base_color[2] * vertex_color[2],
                        ],
                        roughness: material.roughness.clamp(0.04, 1.0),
                        metallic: material.metallic.clamp(0.0, 1.0),
                    });
                }
                for chunk in indices.chunks_exact(3) {
                    if chunk.iter().any(|&i| i >= positions.len()) {
                        continue;
                    }
                    triangles_out.push([
                        vertex_base + chunk[0] as u32,
                        vertex_base + chunk[1] as u32,
                        vertex_base + chunk[2] as u32,
                    ]);
                }
            }
        }
        for child in node["children"].as_array().into_iter().flatten() {
            if let Some(i) = child.as_u64() {
                visit(
                    json,
                    bin,
                    materials,
                    i as usize,
                    world,
                    vertices_out,
                    triangles_out,
                );
            }
        }
    }
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for root in json["scenes"][json["scene"].as_u64().unwrap_or(0) as usize]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
    {
        visit(
            &json,
            bin,
            &materials,
            root.as_u64().unwrap() as usize,
            identity,
            &mut vertices,
            &mut triangles,
        );
    }
    if triangles.is_empty() {
        return Err("模型没有可显示的三角形".into());
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for indices in &triangles {
        for &vertex_index in indices {
            let v = vertices[vertex_index as usize];
            for i in 0..3 {
                min[i] = min[i].min(v.position[i]);
                max[i] = max[i].max(v.position[i]);
            }
        }
    }
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let extent = (0..3)
        .map(|i| max[i] - min[i])
        .fold(0.0, f32::max)
        .max(0.001);
    for v in &mut vertices {
        for i in 0..3 {
            v.position[i] = (v.position[i] - center[i]) * (4.0 / extent);
        }
    }
    Ok(Scene { vertices, triangles })
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn bundled_wunder_model_preserves_scene_scale_and_geometry() {
        let scene = parse(include_bytes!("../../../assets/models/wunder.glb"))
            .expect("bundled GLB should parse");
        assert_eq!(scene.triangles.len(), 312_598);
        assert!(scene.vertices.len() < scene.triangles.len() * 3);
        let max_coordinate = scene
            .vertices
            .iter()
            .flat_map(|vertex| vertex.position)
            .map(f32::abs)
            .fold(0.0, f32::max);
        assert!(
            max_coordinate <= 2.1,
            "normalized model exceeds expected extent"
        );
        assert!(scene
            .vertices
            .iter()
            .any(|vertex| vertex.color != [0.55, 0.58, 0.62]));
    }
}
