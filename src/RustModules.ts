import { dlopen, FFIType, type Pointer, toArrayBuffer } from "bun:ffi";
import { join } from "path";
import { platform } from "os";

let libName = "sparkling_osm_router.so";
if (platform() === "darwin") libName = "sparkling_osm_router.dylib";
else if (platform() === "win32") libName = "sparkling_osm_router.dll";

const libPath = join(import.meta.dir, "..", "target", "release", libName);

export const rustLib = dlopen(libPath, {
    sparkling_build_graph: {
        args: [FFIType.cstring],
        returns: FFIType.bool,
    },
    sparkling_mmap_init: {
        args: [FFIType.ptr, FFIType.u64],
        returns: FFIType.ptr,
    },
    sparkling_mmap_destroy: {
        args: [FFIType.ptr],
        returns: FFIType.void,
    },
    sparkling_find_nearest_node: {
        args: [FFIType.ptr, FFIType.f32, FFIType.f32, FFIType.f32],
        returns: FFIType.u32,
    },
    sparkling_find_route: {
        args: [
            FFIType.ptr, // graph_ptr
            FFIType.u32, // from_idx
            FFIType.u32, // to_idx
            FFIType.u32, // step_limit
            FFIType.ptr, // out_len
            FFIType.ptr, // out_capacity
            FFIType.ptr, // out_error
        ],
        returns: FFIType.ptr,
    },
    sparkling_free_route_result: {
        args: [FFIType.ptr, FFIType.u32, FFIType.u32],
        returns: FFIType.void,
    },
    sparkling_get_nodes_base_ptr: {
        args: [FFIType.ptr],
        returns: FFIType.ptr,
    },
    sparkling_get_nodes_count: {
        args: [FFIType.ptr],
        returns: FFIType.u32,
    },
    sparkling_get_node_tags_json: {
        args: [FFIType.ptr, FFIType.u32, FFIType.ptr],
        returns: FFIType.ptr,
    },
    sparkling_free_string: {
        args: [FFIType.ptr, FFIType.u32],
        returns: FFIType.void,
    },
});

export function readU32ArrayAndFree(pointer: Pointer | null, len: number, capacity: number): number[] {
    if (!pointer || len === 0) return [];

    const buffer = toArrayBuffer(pointer, 0, len * 4);
    const view = new DataView(buffer);

    const result: number[] = [];
    for (let i = 0; i < len; i++) {
        result.push(view.getUint32(i * 4, true));
    }

    rustLib.symbols.sparkling_free_route_result(pointer, len, capacity);
    return result;
}
