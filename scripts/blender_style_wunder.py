import bpy
import bpy
import os
import sys
from mathutils import Vector

SOURCE = sys.argv[-1]
OUTPUT = sys.argv[-2]
PREVIEW = sys.argv[-3]

def material(name, color, metallic, roughness, emission=None):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    m.diffuse_color = (*color, 1)
    p = next((n for n in m.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
    if p is None:
        m.node_tree.nodes.clear()
        p = m.node_tree.nodes.new('ShaderNodeBsdfPrincipled')
        out = m.node_tree.nodes.new('ShaderNodeOutputMaterial')
        m.node_tree.links.new(p.outputs['BSDF'], out.inputs['Surface'])
    p.inputs['Base Color'].default_value = (*color, 1)
    p.inputs['Metallic'].default_value = metallic
    p.inputs['Roughness'].default_value = roughness
    if emission and 'Emission Color' in p.inputs:
        p.inputs['Emission Color'].default_value = (*emission, 1)
        p.inputs['Emission Strength'].default_value = 1.4
    return m

bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
bpy.ops.import_scene.gltf(filepath=SOURCE)
meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH']
# The reference art uses restrained, hand-painted blue grey with charcoal
# machinery, an oxide-red central spine and small brass/energy accents. Each
# family has four related tones. Face orientation chooses among these tones,
# which gives the low-poly surfaces deliberate panel depth without random
# colour noise.
families = {
    'hull': [
        material('Wunder Hull Shadow', (0.12, 0.18, 0.26), .12, .58),
        material('Wunder Hull Blue Grey', (0.23, 0.33, 0.44), .10, .54),
        material('Wunder Hull Light', (0.39, 0.49, 0.59), .08, .57),
        material('Wunder Hull Edge', (0.28, 0.40, 0.52), .12, .49),
    ],
    'graphite': [
        material('Wunder Graphite Shadow', (0.035, 0.050, 0.072), .20, .53),
        material('Wunder Graphite', (0.09, 0.13, 0.18), .18, .50),
        material('Wunder Graphite Light', (0.20, 0.27, 0.33), .14, .53),
        material('Wunder Graphite Edge', (0.14, 0.20, 0.27), .22, .45),
    ],
    'red': [
        material('Wunder Oxide Shadow', (0.11, 0.035, 0.030), .08, .61),
        material('Wunder Oxide Red', (0.27, 0.075, 0.060), .06, .58),
        material('Wunder Oxide Light', (0.43, 0.15, 0.115), .05, .61),
        material('Wunder Oxide Edge', (0.34, 0.10, 0.080), .08, .53),
    ],
    'brass': [
        material('Wunder Brass Shadow', (0.20, 0.095, 0.012), .42, .45),
        material('Wunder Brass Warning', (0.66, 0.38, 0.045), .46, .39),
        material('Wunder Brass Light', (0.92, 0.68, 0.16), .50, .34),
        material('Wunder Brass Edge', (0.78, 0.52, 0.085), .54, .32),
    ],
    'energy': [
        material('Wunder Energy Shadow', (0.015, 0.075, 0.12), .12, .40, (0.002, 0.012, 0.025)),
        material('Wunder Energy Blue', (0.025, 0.22, 0.38), .10, .36, (0.006, 0.055, 0.10)),
        material('Wunder Energy Light', (0.10, 0.55, 0.75), .08, .32, (0.02, 0.20, 0.30)),
        material('Wunder Energy Edge', (0.04, 0.36, 0.56), .12, .34, (0.01, 0.10, 0.18)),
    ],
}

# Stable object-to-family mapping keeps mirrored parts in the same palette.
family_by_object = {
    0: 'graphite', 1: 'graphite', 2: 'hull', 3: 'red', 4: 'graphite',
    5: 'hull', 6: 'graphite', 7: 'brass', 8: 'graphite', 9: 'brass',
    10: 'brass', 11: 'graphite', 12: 'brass', 13: 'graphite',
    # The underside rib cage is the long, high-density 14th mesh in the
    # Sketchfab source. Keep it in the warm gold family used by the reference
    # art; adjacent hull and spine meshes retain their blue-grey/red coatings.
    14: 'brass', 15: 'hull', 16: 'red', 17: 'hull', 18: 'graphite',
}

def assign_materials(obj, index):
    family_name = family_by_object.get(index, 'graphite')
    palette = families[family_name]
    obj.data.materials.clear()
    for item in palette:
        obj.data.materials.append(item)
    # A fixed world-space key gives paired parts the same shading bands. The
    # fourth tone is reserved for grazing faces, preserving a hand-painted
    # edge stripe without introducing a checkerboard pattern.
    key = Vector((0.30, -0.82, 0.48)).normalized()
    for poly in obj.data.polygons:
        normal = (obj.matrix_world.to_3x3() @ poly.normal).normalized()
        facing = normal.dot(key)
        if facing < -0.35:
            poly.material_index = 0
        elif facing > 0.55:
            poly.material_index = 2
        elif abs(facing) < 0.16:
            poly.material_index = 3
        else:
            poly.material_index = 1

for i, obj in enumerate(meshes):
    for attribute in list(obj.data.color_attributes):
        obj.data.color_attributes.remove(attribute)
    assign_materials(obj, i)
    for poly in obj.data.polygons:
        poly.use_smooth = True

points = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
lo = Vector((min(v.x for v in points), min(v.y for v in points), min(v.z for v in points)))
hi = Vector((max(v.x for v in points), max(v.y for v in points), max(v.z for v in points)))
center = (lo + hi) * .5
for obj in meshes:
    obj.location -= center
points = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
lo = Vector((min(v.x for v in points), min(v.y for v in points), min(v.z for v in points)))
hi = Vector((max(v.x for v in points), max(v.y for v in points), max(v.z for v in points)))
center = (lo + hi) * .5
radius = (hi - lo).length * .5

bpy.context.scene.render.engine = 'BLENDER_EEVEE'
bpy.context.scene.render.resolution_x = 1100
bpy.context.scene.render.resolution_y = 760
bpy.context.scene.render.resolution_percentage = 100
bpy.context.scene.render.image_settings.file_format = 'PNG'
bpy.context.scene.render.filepath = PREVIEW
bpy.context.scene.world.color = (.015, .022, .030)
bpy.context.scene.view_settings.view_transform = 'Standard'
bpy.context.scene.view_settings.look = 'Medium High Contrast'
bpy.context.scene.view_settings.exposure = -0.35
cam_data = bpy.data.cameras.new('Wunder Preview Camera')
cam = bpy.data.objects.new('Wunder Preview Camera', cam_data)
bpy.context.collection.objects.link(cam)
cam.location = center + Vector((.02, -radius * 2.15, .01))
cam_data.lens = 58
cam.rotation_euler = (center - cam.location).to_track_quat('-Z', 'Y').to_euler()
bpy.context.scene.camera = cam
for name, location, power, size in [('Key', (radius*1.8, -radius*1.3, radius*1.7), 520, radius), ('Rim', (-radius*1.5, radius, radius*1.2), 420, radius*.8), ('Fill', (radius*.3, radius*.5, -radius*1.5), 280, radius*.6)]:
    data = bpy.data.lights.new(name, 'AREA'); data.energy = power; data.shape = 'DISK'; data.size = size
    light = bpy.data.objects.new(name, data); bpy.context.collection.objects.link(light); light.location = location
    light.rotation_euler = (center - light.location).to_track_quat('-Z', 'Y').to_euler()
bpy.ops.wm.save_as_mainfile(filepath=os.path.splitext(OUTPUT)[0] + '.blend')
bpy.ops.render.render(write_still=True)
bpy.ops.export_scene.gltf(filepath=OUTPUT, export_format='GLB', export_materials='EXPORT', export_cameras=False, export_lights=False)
print('WUNDER_STYLE_EXPORTED', OUTPUT)
