"""
GLB -> FBX 일괄 변환.

Mixamo는 GLB를 받지 않는다. 배포 중인 리그를 Mixamo에 올리려면 FBX가 필요한데
`assets/`에는 bugbear/ogre/troll/stone_golem 정도만 FBX가 남아 있어서,
`client/public/models/`의 GLB에서 다시 뽑는다.

사용법:
  Blender --background --python tools/blender-scripts/glb_to_fbx.py -- <출력폴더> [--mesh-only] <glb...>

--mesh-only: 아마추어와 애니메이션을 빼고 메시만 내보낸다. Mixamo auto-rig는
리그가 없는 메시를 원하므로 업로드용은 이쪽.
"""

import sys
from pathlib import Path

import bpy


def drop_gltf_helpers() -> None:
    """glTF 임포터가 만드는 `glTF_not_exported` 컬렉션을 지운다.

    본 커스텀 셰이프용 Icosphere가 들어 있다. glTF 익스포터는 이름을 보고
    빼지만 FBX 익스포터는 모르기 때문에, 지우지 않으면 모든 FBX 원점에
    정체불명의 구가 하나씩 박힌다.
    """
    collection = bpy.data.collections.get("glTF_not_exported")
    if not collection:
        return
    for obj in list(collection.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    bpy.data.collections.remove(collection)


def convert(src: Path, out_dir: Path, mesh_only: bool) -> tuple[str, int, int]:
    # read_homefile, not read_factory_settings: the latter reloads add-ons and
    # the glTF importer stops responding from the second file onward.
    bpy.ops.wm.read_homefile(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(src))
    drop_gltf_helpers()

    for obj in list(bpy.data.objects):
        if obj.type not in {"MESH", "ARMATURE"}:
            bpy.data.objects.remove(obj, do_unlink=True)

    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    actions = len(bpy.data.actions)

    # 이름을 정리한다. glTF 임포터가 노드 이름과 부딪히면 `Armature001`을 만들고,
    # verse8.io 모델은 `node_178b04ec-...-target_6a283dd2` 같은 이름을 달고 온다.
    for obj in bpy.data.objects:
        if obj.type == "ARMATURE":
            obj.name = "Armature"
    for index, mesh in enumerate(meshes):
        mesh.name = src.stem if index == 0 else f"{src.stem}_{index}"

    if mesh_only:
        for obj in list(bpy.data.objects):
            if obj.type != "MESH":
                bpy.data.objects.remove(obj, do_unlink=True)
        for obj in meshes:
            for mod in list(obj.modifiers):
                if mod.type == "ARMATURE":
                    obj.modifiers.remove(mod)
        for action in list(bpy.data.actions):
            bpy.data.actions.remove(action)

    dest = out_dir / f"{src.stem}.fbx"
    bpy.ops.export_scene.fbx(
        filepath=str(dest),
        path_mode="COPY",
        embed_textures=True,
        add_leaf_bones=False,
        bake_anim=not mesh_only,
        object_types={"MESH"} if mesh_only else {"ARMATURE", "MESH"},
        use_mesh_modifiers=False,
    )
    return dest.name, len(meshes), actions


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :]
    out_dir = Path(argv[0]).expanduser()
    mesh_only = "--mesh-only" in argv
    sources = [Path(a) for a in argv[1:] if not a.startswith("--")]

    out_dir.mkdir(parents=True, exist_ok=True)
    failed = []
    for src in sources:
        try:
            name, meshes, actions = convert(src, out_dir, mesh_only)
        except Exception as error:
            failed.append(src.name)
            print(f"FAIL {src.name} {error}", flush=True)
            continue
        size = (out_dir / name).stat().st_size / 1048576
        print(f"OK {name} {size:.1f}MB meshes={meshes} clips={actions}", flush=True)
    if failed:
        print(f"FAILED {len(failed)}: {', '.join(failed)}", flush=True)


main()
