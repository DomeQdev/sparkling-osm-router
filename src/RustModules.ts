import { Location, OsmNode, OsmWay, QueueStatus, RouteResult } from "./typings";

const binding = require("../index.node");

export const loadGraph: (path: string) => number = binding.loadGraph;
export const createProfile: (profile: string) => number = binding.createProfile;
export const unloadGraph: (graphId: number) => boolean = binding.unloadGraph;
export const getNode: (graphId: number, node: number) => OsmNode | null = binding.getNode;
export const getWay: (graphId: number, way: number) => OsmWay | null = binding.getWay;
export const getShape: (graphId: number, nodes: number[]) => Location[] = binding.getShape;

export const getNearestNodes: (
    graphId: number,
    profileId: number,
    lon: number,
    lat: number,
    limit: number,
    maxDistanceThreshold: number
) => number[] = binding.getNearestNodes;

export const getRoute: (
    graphId: number,
    profileId: number,
    startNode: number,
    endNode: number
) => Promise<RouteResult> = binding.getRoute;

export const createRouteQueue: (graphId: number, profileId: number, maxConcurrency?: number) => number =
    binding.createRouteQueue;

export const enqueueRoute: (queueId: number, routeId: string, startNode: number, endNode: number) => string =
    binding.enqueueRoute;

export const startQueueProcessing: (
    queueId: number,
    callback: (id: string, result: RouteResult | Error | null) => void
) => void = binding.startQueueProcessing;

export const getQueueStatus: (queueId: number) => QueueStatus = binding.getQueueStatus;
export const clearRouteQueue: (queueId: number) => boolean = binding.clearRouteQueue;
