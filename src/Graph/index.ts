/**
 * Graph module for OSM-based routing operations.
 * Provides functionality for loading OSM graphs, finding routes, and managing graph data.
 * @module Graph
 */
import {
    cleanupGraphStore,
    createGraphStore,
    findNearestNode,
    getNode,
    getShape as getShapeRust,
    getWay,
    loadAndIndexGraph,
    offsetPoints,
    route,
    simplifyShape as simplifyShapeRust,
} from "../RustModules";
import loadOSMGraph from "./loadOSMGraph";

export type Location = [number, number];

/**
 * Configuration for a routing profile.
 */
export type Profile = {
    /**
     * The OSM tag key to consider for routing (e.g., "highway").
     * See https://wiki.openstreetmap.org/wiki/Tags#Keys_and_values
     */
    key: string;

    /**
     * Map of penalties for different OSM tag values (e.g., {"motorway": 1, "residential": 3}),
     * including an optional default value for tags not explicitly specified.
     * If default is not provided, routing will occur ONLY on ways with explicitly specified tags.
     */
    penalties: Partial<Record<string | "default", number>>;

    /**
     * The type of vehicle used for routing, which affects access restrictions and turn restrictions.
     * Possible values: "foot", "bicycle", "motorcar", "motorcycle", "psv", "train", "subway", "tram"
     * If not provided, no vehicle-specific filtering will be applied.
     */
    vehicle_type?: "foot" | "bicycle" | "motorcar" | "motorcycle" | "psv" | "train" | "subway" | "tram";
};

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

/**
 * Graph class that provides routing functionality using OSM data.
 * It handles loading graph data, finding routes, and managing graph nodes and ways.
 */
class Graph {
    private options: GraphOptions;
    private graph: number | null = null;

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

        loadAndIndexGraph(this.options.osmGraph.path, this.graph, JSON.stringify(this.options.profile));
    };

    /**
     * Calculates a route between two nodes.
     * @param startNode - The ID of the starting node
     * @param endNode - The ID of the ending node
     * @param bearing - Optional bearing direction in degrees
     * @returns The calculated route result
     * @throws If the graph is not loaded
     */
    getRoute = async (startNode: number, endNode: number, bearing?: number): Promise<RouteResult> => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        return route(startNode, endNode, bearing ?? null, this.graph);
    };

    /**
     * Finds the nearest node to given coordinates.
     * @param location - Longitude and latitude coordinates
     * @returns The ID of the nearest node
     * @throws If the graph is not loaded
     */
    getNearestNode = (location: Location) => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        const [lon, lat] = location;

        return findNearestNode(lon, lat, this.graph);
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
     * Cleans up the graph resources and resets the graph state.
     * @returns Result of the cleanup operation
     */
    cleanup = (): boolean => {
        this.graph = null;
        return cleanupGraphStore();
    };
}

export default Graph;
