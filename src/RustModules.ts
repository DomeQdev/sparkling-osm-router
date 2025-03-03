const binding = require("../index.node");
import { Location } from "./typings";

export interface IOsmNodeData {
    id: number;
    lat: number;
    lon: number;
    tags: Record<string, string>;
}

export interface IOsmWayData {
    id: number;
    nodes: number[];
    tags: Record<string, string>;
}

export const createGraphStore: () => number = binding.createGraphStore;

export const loadAndIndexGraph: (path: string, graphId: number, profile: string) => boolean =
    binding.loadAndIndexGraph;

export const findNearestNode: (
    lon: number,
    lat: number,
    graphId: number,
    usePenalties?: boolean
) => number | null = binding.findNearestNode;

export const route: (
    startNode: number,
    endNode: number,
    initialBearing: number | null,
    graphId: number
) => Promise<{ nodes: number[]; ways: number[] }> = binding.route;

export const getNode: (node: number, graphId: number) => IOsmNodeData | null = binding.getNode;

export const getWay: (way: number, graphId: number) => IOsmWayData | null = binding.getWay;

export const getWayForNodes: (nodes: number[], graphId: number) => number[] = binding.getWayForNodes;

export const offsetShape: (
    graphId: number,
    nodes: number[],
    offsetMeters: number,
    offsetSide: 1 | -1
) => Location[] = binding.offsetRouteShape;

export const cleanupGraphStore: () => boolean = binding.cleanupGraphStore;
