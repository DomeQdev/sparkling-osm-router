import { Location, RouteResult } from "./Graph";

const binding = require("../index.node");

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

export interface SearchNodeResult {
    id: number;
    score: number;
}

export const createGraphStore: () => number = binding.createGraphStore;

export const loadAndIndexGraph: (path: string, graphId: number, profile: string) => boolean =
    binding.loadAndIndexGraph;

export const findNearestNode: (
    lon: number,
    lat: number,
    graphId: number,
    limit?: number,
    maxDistance?: number
) => number[] = binding.findNearestNode;

export const searchNearestNode: (
    lon: number,
    lat: number,
    searchString: string,
    maxDistance: number,
    graphId: number
) => number = binding.searchNearestNode;

export const route: (
    startNode: number,
    endNode: number,
    bearing: number | null,
    graphId: number
) => Promise<RouteResult> = (startNode, endNode, bearing, graphId) =>
    binding.route(startNode, endNode, bearing, graphId);

export const getNode: (node: number, graphId: number) => IOsmNodeData | null = binding.getNode;

export const getWay: (way: number, graphId: number) => IOsmWayData | null = binding.getWay;

export const getShape: (graphId: number, nodes: number[]) => Location[] = binding.getShape;

export const simplifyShape: (graphId: number, nodes: number[], epsilon: number) => Location[] =
    binding.simplifyShape;

export const offsetPoints: (points: Location[], offsetMeters: number, offsetSide: 1 | -1) => Location[] =
    binding.offsetPoints;

export const cleanupGraphStore: () => boolean = binding.cleanupGraphStore;

export const createRouteQueue: (graphId: number, maxConcurrency?: number) => number =
    binding.createRouteQueue;

export const enqueueRoute: (
    queueId: number,
    routeId: string,
    startNode: number,
    endNode: number,
    initialBearing: number | null
) => string = binding.enqueueRoute;

export const startQueueProcessing: (
    queueId: number,
    callback: (id: string, result: RouteResult | null | Error) => void
) => void = binding.startQueueProcessing;

export interface QueueStatus {
    queuedTasks: number;
    activeTasks: number;
    isEmpty: boolean;
}

export const getQueueStatus: (queueId: number) => QueueStatus = binding.getQueueStatus;

export const clearRouteQueue: (queueId: number) => boolean = binding.clearRouteQueue;

export const cleanupRouteQueue: (queueId: number) => boolean = binding.cleanupRouteQueue;
