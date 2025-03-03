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
    getWay,
    loadAndIndexGraph,
    offsetShape,
    route,
} from "../RustModules";
import { GraphOptions, Location, RouteResult } from "../typings";
import loadOSMGraph from "./loadOSMGraph";

/**
 * Graph class that provides routing functionality using OSM data.
 * It handles loading graph data, finding routes, and managing graph nodes and ways.
 */
class Graph {
    private options: GraphOptions;
    private graph: number | null = null;

    /**
     * Creates a new Graph instance.
     * @param {GraphOptions} options - Configuration options for the graph
     */
    constructor(options: GraphOptions) {
        this.options = options;
    }

    /**
     * Loads the OSM graph data into memory.
     * @returns {Promise<void>} A promise that resolves when the graph has been loaded
     */
    loadGraph = async () => {
        await loadOSMGraph(this.options.osmGraph);

        this.graph = createGraphStore();

        loadAndIndexGraph(this.options.osmGraph.path, this.graph, JSON.stringify(this.options.profile));
    };

    /**
     * Calculates a route between two nodes.
     * @param {number} startNode - The ID of the starting node
     * @param {number} endNode - The ID of the ending node
     * @param {number} [bearing] - Optional bearing direction in degrees
     * @returns {Promise<RouteResult>} The calculated route result
     * @throws {Error} If the graph is not loaded
     */
    getRoute = async (startNode: number, endNode: number, bearing?: number): Promise<RouteResult> => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        return route(startNode, endNode, bearing ?? null, this.graph);
    };

    /**
     * Finds the nearest node to given coordinates.
     * @param {Location} [lon, lat] - Longitude and latitude coordinates
     * @param {boolean} usePenalties - Whether to apply penalties during search (default: true)
     * @returns {number} The ID of the nearest node
     * @throws {Error} If the graph is not loaded
     */
    getNearestNode = ([lon, lat]: Location, usePenalties: boolean = true) => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        return findNearestNode(lon, lat, this.graph, usePenalties);
    };

    /**
     * Gets node data for a list of node IDs.
     * @param {{ nodes: number[] }} param0 - Object containing array of node IDs
     * @returns {Array} Array of node data objects
     * @throws {Error} If the graph is not loaded
     */
    getNodes = ({ nodes }: { nodes: number[] }) => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        return nodes.map((node) => getNode(node, this.graph!)!);
    };

    /**
     * Gets way data for a list of way IDs.
     * @param {{ ways: number[] }} param0 - Object containing array of way IDs
     * @returns {Array} Array of way data objects
     * @throws {Error} If the graph is not loaded
     */
    getWays = ({ ways }: { ways: number[] }) => {
        if (this.graph === null) throw new Error("Graph is not loaded");

        return ways.map((way) => getWay(way, this.graph!)!);
    };

    /**
     * Returns the geographic shape formed by a list of nodes.
     * @param {{ nodes: number[] }} param0 - Object containing array of node IDs
     * @returns {Location[]} Array of [longitude, latitude] coordinates
     */
    getShape = ({ nodes }: { nodes: number[] }) => {
        const nodeData = this.getNodes({ nodes });

        return nodeData.map((node) => [node.lon, node.lat]);
    };

    /**
     * Gets an offset shape for a list of nodes, useful for drawing paths with offsets.
     * @param {{ nodes: number[] }} param0 - Object containing array of node IDs
     * @param {number} offsetMeters - Offset distance in meters (default: 1.5)
     * @param {1 | -1} offsetSide - Side of the offset, 1 for right, -1 for left (default: 1)
     * @returns {Location[]} Array of offset [longitude, latitude] coordinates
     * @throws {Error} If the graph is not loaded or node list is empty
     */
    getOffsetShape = (
        { nodes }: { nodes: number[] },
        offsetMeters: number = 1.5,
        offsetSide: 1 | -1 = 1
    ): Location[] => {
        if (!this.graph) throw new Error("Graph is not loaded");
        if (!nodes.length) return [];

        return offsetShape(this.graph, nodes, offsetMeters, offsetSide);
    };

    /**
     * Cleans up the graph resources and resets the graph state.
     * @returns {boolean} Result of the cleanup operation
     */
    cleanup = () => {
        this.graph = null;
        return cleanupGraphStore();
    };
}

export default Graph;
