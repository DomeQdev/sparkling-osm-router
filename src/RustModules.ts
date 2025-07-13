import { Location, OsmNode, QueueStatus, RouteResult } from "./typings";

const binding = require("../index.node");

export const loadGraph: (optionsJson: string) => number = binding.loadGraph;
export const unloadGraph: (graphId: number) => boolean = binding.unloadGraph;

export const getNode: (graphId: number, profileId: string, nodeId: number) => OsmNode | null =
    binding.getNode;

export const getShape: (graphId: number, profileId: string, nodes: number[]) => Location[] = binding.getShape;

export const getNearestNode: (graphId: number, profileId: string, lon: number, lat: number) => number | null =
    binding.getNearestNode;

export const getNodesInRadius: (
    graphId: number,
    profileId: string,
    lon: number,
    lat: number,
    radiusMeters: number
) => OsmNode[] = binding.getNodesInRadius;

export const getRoute: (
    graphId: number,
    profileId: string,
    startNode: number,
    endNode: number
) => Promise<RouteResult | null> = binding.getRoute;

export const createRouteQueue: (graphId: number, profileId: string, maxConcurrency?: number) => number =
    binding.createRouteQueue;

export const enqueueRoute: (queueId: number, routeId: string, startNode: number, endNode: number) => string =
    binding.enqueueRoute;

export const processQueue: (
    queueId: number,
    callback: (id: string, result: RouteResult | Error | null) => void
) => void = binding.processQueue;

export const getQueueStatus: (queueId: number) => QueueStatus = binding.getQueueStatus;

export const clearRouteQueue: (queueId: number) => boolean = binding.clearRouteQueue;
