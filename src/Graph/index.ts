/**
 * Graph module for OSM-based routing operations.
 * Provides functionality for loading OSM graphs, finding routes, and managing graph data.
 * @module Graph
 */
import {
    cleanupGraphStore,
    cleanupRouteQueue,
    createGraphStore,
    findNearestNode,
    getNode,
    getShape as getShapeRust,
    getWay,
    loadAndIndexGraph,
    offsetPoints,
    route,
    searchNearestNode as searchNearestNodeRust,
    simplifyShape as simplifyShapeRust,
} from "../RustModules";
import loadOSMGraph from "./loadOSMGraph";
import { convertProfileFormat, Profile } from "./Profile";
import { RouteQueue } from "./RouteQueue";

export type Location = [number, number];

/**
 * Configuration options for the routing graph.
 */
export type GraphOptions = {
    /**
     * Options related to fetching data from OpenStreetMap.
     */
    osmGraph: OSMGraphOptions;

    /**
     * Profile used for route calculations.
     */
    profile: Profile;
};

/**
 * Configuration options for fetching OpenStreetMap data.
 */
export type OSMGraphOptions = {
    /**
     * Path to the OSM data file.
     */
    path: string;

    /**
     * Time to live for cached data in hours.
     */
    ttl: number;

    /**
     * Geographic boundaries of the query area.
     */
    bounds: Location[];

    /**
     * Query for the Overpass API.
     */
    overpassQuery: string;
};

/**
 * Result of a routing process.
 */
export type RouteResult = {
    /**
     * List of node IDs that form the route.
     */
    nodes: number[];

    /**
     * List of way IDs that form the route.
     */
    ways: number[];
};

export { RouteQueue };

/**
 * Graph class that provides routing functionality using OSM data.
 * It handles loading graph data, finding routes, and managing graph nodes and ways.
 */
class Graph {
    static RouteQueue = RouteQueue;

    private options: GraphOptions;
    private graph: number | null = null;
    private queueIds: number[] = [];

    /**
     * Creates a new Graph instance.
     * @param options - Configuration options for the graph
     */
    constructor(options: GraphOptions) {
        this.options = options;
    }

    /**
     * Loads the OSM graph data into memory.
     * @returns A promise that resolves when the graph has been loaded
     */
    loadGraph = async () => {
        await loadOSMGraph(this.options.osmGraph);

        this.graph = createGraphStore();

        loadAndIndexGraph(this.options.osmGraph.path, this.graph, convertProfileFormat(this.options.profile));
    };

    /**
     * Calculates a route between two nodes.
     * @param startNode - ID of the starting node
     * @param endNode - ID of the ending node
     * @param bearing - Optional bearing direction in degrees
     * @returns The calculated route result
     * @throws If the graph is not loaded
     */
    getRoute = async (startNode: number, endNode: number, bearing?: number): Promise<RouteResult> => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        return route(startNode, endNode, bearing ?? null, this.graph);
    };

    /**
     * Searches for a node near the given coordinates that best matches the search string in its tags.
     * @param location - Longitude and latitude coordinates
     * @param searchString - String to search for in node tags
     * @param searchLimit - Maximum number of nodes to search (default: 25)
     * @param distanceThresholdMultiplier - Multiplier for distance threshold (default: 25.0)
     * @returns ID of the nearest node that matches the search string
     * @throws If the graph is not loaded
     */
    searchNearestNode = (location: Location, searchString: string, searchLimit: number = 25, distanceThresholdMultiplier: number = 25.0) => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        const [lon, lat] = location;

        return searchNearestNodeRust(lon, lat, searchString, searchLimit, distanceThresholdMultiplier, this.graph);
    };

    /**
     * Finds the nearest nodes to given coordinates.
     * @param location - Longitude and latitude coordinates
     * @param limit - Maximum number of nodes to return (default: 1)
     * @param distanceThresholdMultiplier - Multiplier for distance threshold when selecting multiple nodes (default: 5.0)
     * @returns Array of IDs of the nearest nodes
     * @throws If the graph is not loaded
     */
    getNearestNodes = (location: Location, limit: number = 1, distanceThresholdMultiplier: number = 5.0) => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        const [lon, lat] = location;

        return findNearestNode(lon, lat, this.graph, limit, distanceThresholdMultiplier);
    };

    /**
     * Finds the nearest node to given coordinates.
     * @param location - Longitude and latitude coordinates
     * @returns ID of the nearest node
     * @throws If the graph is not loaded
     */
    getNearestNode = (location: Location) => {
        return this.getNearestNodes(location)?.[0];
    };

    /**
     * Gets node data for a list of node IDs.
     * @param nodes Object containing array of node IDs
     * @returns Array of node data objects
     * @throws If the graph is not loaded
     */
    getNodes = ({ nodes }: { nodes: number[] }) => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        return nodes.map((node) => getNode(node, this.graph!)!);
    };

    /**
     * Gets way data for a list of way IDs.
     * @param ways - Object containing array of way IDs
     * @returns Array of way data objects
     * @throws If the graph is not loaded
     */
    getWays = ({ ways }: { ways: number[] }) => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        return ways.map((way) => getWay(way, this.graph!)!);
    };

    /**
     * Returns the geographic shape formed by a list of nodes.
     * @param nodes - Object containing array of node IDs
     * @returns Array of [longitude, latitude] coordinates
     */
    getShape = ({ nodes }: { nodes: number[] }): Location[] => {
        if (this.graph === null) throw new Error("Graph is not loaded");
        if (!nodes.length) return [];

        return getShapeRust(this.graph, nodes);
    };

    /**
     * Returns a simplified version of a route shape using the Ramer-Douglas-Peucker algorithm.
     * @param nodes - Object containing array of node IDs
     * @param epsilon - Simplification tolerance value (higher value = more simplification)
     * @returns Array of simplified [longitude, latitude] coordinates
     */
    getSimplifiedShape = ({ nodes }: { nodes: number[] }, epsilon: number = 1e-5): Location[] => {
        if (this.graph === null) throw new Error("Graph is not loaded");
        if (!nodes.length) return [];

        return simplifyShapeRust(this.graph, nodes, epsilon);
    };

    /**
     * Gets an offset shape for a list of coordinates, useful for drawing paths with offsets.
     * @param shape - Array of [longitude, latitude] coordinates
     * @param offsetMeters - Offset distance in meters (default: 1.5)
     * @param offsetSide - Side of the offset, 1 for right, -1 for left (default: 1)
     * @returns Array of offset [longitude, latitude] coordinates
     */
    offsetShape = (shape: Location[], offsetMeters: number = 1.5, offsetSide: 1 | -1 = 1): Location[] => {
        if (!shape.length) return [];

        return offsetPoints(shape, offsetMeters, offsetSide);
    };

    /**
     * Creates a new route queue associated with this graph.
     * The graph must be loaded first.
     * @param enableProgessBar Whether to enable progress bar for route calculations
     * @param maxConcurrency Optional maximum number of concurrent route calculations
     * @returns A new RouteQueue instance
     * @throws If the graph is not loaded
     */
    createRouteQueue(enableProgessBar: boolean = true, maxConcurrency?: number): RouteQueue {
        if (this.graph === null) throw new Error("Graph is not loaded");

        const queue = new RouteQueue(this.graph, enableProgessBar, maxConcurrency);
        this.queueIds.push(queue.queueId);

        return queue;
    }

    /**
     * Cleans up the graph resources and resets the graph state.
     * @returns Result of the cleanup operation
     */
    cleanup = (): boolean => {
        this.graph = null;

        for (const queueId of this.queueIds) {
            cleanupRouteQueue(queueId);
        }

        return cleanupGraphStore();
    };
}

export default Graph;
