import { readU32ArrayAndFree, rustLib } from "./RustModules";
import type { Location, ProfileConfig, RouteResult } from "./typings";
import { type Pointer, ptr, toArrayBuffer } from "bun:ffi";

export type BuildGraphOptions = {
    osmPath: string;
    outPath: string;
    profile: ProfileConfig;
    tagFilters?: { key: string; value: string; tagsToSave: string[] }[];
};

class Graph {
    public graphPointer: Pointer | null = null;
    private nodesBasePointer: Pointer | null = null;
    private nodesCount: number = 0;

    private fileBuffer: Uint8Array | null = null;

    constructor() {}

    loadGraph = (buffer: SharedArrayBuffer) => {
        if (this.graphPointer !== null) return;

        this.fileBuffer = new Uint8Array(buffer);
        this.graphPointer = rustLib.symbols.sparkling_mmap_init(
            ptr(this.fileBuffer),
            this.fileBuffer.byteLength,
        );

        if (!this.graphPointer) {
            throw new Error("Failed to initialize MemoryMappedGraph from buffer.");
        }

        this.nodesBasePointer = rustLib.symbols.sparkling_get_nodes_base_ptr(this.graphPointer);
        this.nodesCount = rustLib.symbols.sparkling_get_nodes_count(this.graphPointer);
    };

    unloadGraph = () => {
        if (!this.graphPointer) return false;

        rustLib.symbols.sparkling_mmap_destroy(this.graphPointer);
        this.graphPointer = null;
        this.nodesBasePointer = null;
        this.nodesCount = 0;
        this.fileBuffer = null;
        return true;
    };

    public static buildGraphFromOsm = ({ profile, osmPath, outPath, tagFilters }: BuildGraphOptions) => {
        const jsonOptions = {
            osm_path: osmPath,
            out_path: outPath,
            profile: {
                name: profile.id,
                penalties: profile.penalties.map(([k, penalty]) => ({
                    key: profile.key,
                    value: Array.isArray(k) ? k[0] : k,
                    penalty,
                })),
                access: Array.from(new Set([...(profile.accessTags ?? []), "access"])),
                disallow_motorroad: profile.disallowMotorroad ?? false,
                disable_restrictions: profile.disableRestrictions ?? false,
            },
            bbox: [0, 0, 0, 0],
            tag_filters: (tagFilters || [])?.map(({ key, value, tagsToSave }) => ({
                key,
                value,
                tags_to_save: tagsToSave,
            })),
        };

        const success = rustLib.symbols.sparkling_build_graph(
            Buffer.from(JSON.stringify(jsonOptions) + "\0"),
        );

        if (!success) {
            throw new Error("Rust core failed to build the binary graph.");
        }
    };

    getNearestNode = (location: Location, searchRadiusMeters: number = 10000): number | null => {
        if (!this.graphPointer) throw new Error("Graph is not loaded");

        const nodeId = rustLib.symbols.sparkling_find_nearest_node(
            this.graphPointer,
            location[1],
            location[0],
            searchRadiusMeters,
        );

        if (nodeId === 4294967295) return null; // u32::MAX
        return nodeId;
    };

    getRoute = async (startNodeId: number, endNodeId: number): Promise<RouteResult | null> => {
        if (!this.graphPointer) throw new Error("Graph is not loaded");

        const outLenPtr = new Uint32Array(1);
        const outCapPtr = new Uint32Array(1);
        const outErrPtr = new Uint8Array(1);

        const routePtr = rustLib.symbols.sparkling_find_route(
            this.graphPointer,
            startNodeId,
            endNodeId,
            2_000_000,
            ptr(outLenPtr),
            ptr(outCapPtr),
            ptr(outErrPtr),
        );

        const errorCode = outErrPtr[0];

        if (errorCode === 1) return null;
        if (errorCode === 2) throw new Error("Invalid Node Reference");
        if (errorCode === 3) throw new Error("Step limit exceeded (route too long)");

        const len = outLenPtr[0];
        const capacity = outCapPtr[0];

        const nodes = readU32ArrayAndFree(routePtr, len!, capacity!);

        return { nodes };
    };

    getShape = ({ nodes }: RouteResult): Location[] => {
        if (!this.nodesBasePointer || this.nodesCount === 0) throw new Error("Graph is not loaded");

        const buf = toArrayBuffer(this.nodesBasePointer, 0, this.nodesCount * 32);
        const view = new DataView(buf);

        const shape: Location[] = [];

        for (const nodeId of nodes) {
            const nodeOffset = nodeId * 32;

            const lat = view.getFloat32(nodeOffset + 8, true);
            const lon = view.getFloat32(nodeOffset + 12, true);

            shape.push([lon, lat]);
        }

        return shape;
    };
}

export default Graph;
