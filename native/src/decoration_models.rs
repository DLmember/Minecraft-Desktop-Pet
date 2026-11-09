//! Resting Bedrock/GeckoLib cuboids, loaded once from the original model assets.
use super::*;
use serde_json::Value;
use std::sync::OnceLock;
pub(super) struct Model { pub parts: Vec<BoxPart>, pub image: RgbaImage, pub colors: Vec<[[u8;3];6]>, pub bottom: f32 }
fn vector(value: &Value) -> Vec3 {
    Vec3::new(value[0].as_f64().unwrap_or(0.) as f32,value[1].as_f64().unwrap_or(0.) as f32,value[2].as_f64().unwrap_or(0.) as f32)
}
fn pivot_rotation(node:&Value)->Mat4 {
    let p=vector(&node["pivot"]);let r=vector(&node["rotation"])*std::f32::consts::PI/180.;
    Mat4::from_translation(p)*Mat4::from_quat(Quat::from_euler(glam::EulerRot::ZYX,r.z,r.y,r.x))*Mat4::from_translation(-p)
}
fn load(geometry:&str,texture:&[u8])->Model {
    let data:Value=serde_json::from_str(geometry).unwrap();let geo=&data["minecraft:geometry"][0];
    let bones=geo["bones"].as_array().unwrap();let mut transforms=std::collections::HashMap::new();
    let mut parts=Vec::new();let conversion=Mat4::from_rotation_y(std::f32::consts::PI);
    for bone in bones {
        let parent=bone["parent"].as_str().and_then(|n|transforms.get(n)).copied().unwrap_or(Mat4::IDENTITY);
        let transform=parent*pivot_rotation(bone);
        transforms.insert(bone["name"].as_str().unwrap(),transform);
        if let Some(cubes)=bone["cubes"].as_array() {for cube in cubes {
            let size=vector(&cube["size"]);let center=vector(&cube["origin"])+size*0.5;
            let uv=&cube["uv"];
            assert!(uv.is_array(),"decorations currently use the original box UV layout");
            parts.push(BoxPart{size,uv:Vec2::new(uv[0].as_f64().unwrap() as f32,uv[1].as_f64().unwrap() as f32),uv_size:None,
                transform:conversion*transform*pivot_rotation(cube)*Mat4::from_translation(center)*conversion,
                inflate:0.,mirror:cube["mirror"].as_bool().unwrap_or(false),keyboard:false,cape:false,faces:None,
                eyes:None,eyelid:1.,pupil:Vec2::ZERO});
        }}
    }
    let image=image::load_from_memory(texture).unwrap().to_rgba8();
    assert_eq!(image.width() as u64,geo["description"]["texture_width"].as_u64().unwrap());
    // Mount using the posed geometry, not the unposed legs' bounding box.
    let bottom=parts.iter().flat_map(|p| {
        [-1.,1.].into_iter().flat_map(move |x| [-1.,1.].into_iter().flat_map(move |y|
            [-1.,1.].into_iter().map(move |z| p.transform.transform_point3(p.size*Vec3::new(x,y,z)*0.5).y)))
    }).fold(f32::INFINITY,f32::min);
    let colors=colors_for(&image);Model{parts,image,colors,bottom}
}
pub(super) fn get(kind:&str)-> &'static Model {
    static AXOLOTL:OnceLock<Model>=OnceLock::new();
    match kind {
        "axolotl"=>AXOLOTL.get_or_init(||load(include_str!("../assets/decorations/axolotl.geo.json"),include_bytes!("../assets/decorations/axolotl_lucy.png"))),
        _=>unreachable!(),
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn axolotl_feet_are_flat_and_mount_clears_outer_skin() {
        let axolotl=get("axolotl");
        for foot in &axolotl.parts[2..6] {
            assert!((foot.transform.transform_vector3(Vec3::Z).y.abs()-1.).abs()<0.0001);
        }
        for (name,scale) in [("axolotl",0.36)] {
            let model=get(name);
            for p in &model.parts {for x in [-1.,1.] {for y in [-1.,1.] {for z in [-1.,1.] {
                let corner=p.transform.transform_point3(p.size*Vec3::new(x,y,z)*0.5);
                assert!(4.55+(corner.y-model.bottom)*scale>4.5);
            }}}}
        }
    }
}
