# takes

Downloads waiting to be auditioned. Drop `.fbx`, `.glb` or `.gltf` in here.

A file in a folder named after a motion is offered for that motion:

    takes/walk/heavy_stride.fbx     -> offered for walk
    takes/walk/patrol.fbx           -> offered for walk
    takes/slash1/overhead.fbx       -> offered for slash1

A file at the root is unfiled, and offered for every motion — drop a download
straight in and try it anywhere:

    takes/some_download.fbx         -> offered everywhere

Download from Mixamo **with skin**. The game's retargeter needs a skinned mesh
on both sides, and a "Without Skin" download has none; the tool says so per take
rather than showing you a preview that did not actually retarget.

Nothing in here is committed except this file.
