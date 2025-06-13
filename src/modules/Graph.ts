import { getNode, getShape, getWay, loadGraph, unloadGraph } from "../RustModules";
import { existsSync, mkdirSync, statSync, writeFileSync } from "fs";
import { Location } from "../typings";
import { dirname } from "path";
import BaseProfile from "./Profile";

export type GraphOptions = {
    filePath: string;
    overpassGraph?: {
        query: string[];
        bounds: Location[];
        ttlDays: number;
        server?: string;
        timeout?: number;
        retries?: number;
        retryDelay?: number;
    };
};

class Graph {
    private options: GraphOptions;
    public graphId: number | null = null;
    public Profile: typeof BaseProfile;

    constructor(options: GraphOptions) {
        const parentGraph = this;
        this.options = options;

        this.Profile = class Profile extends BaseProfile {
            get graph() {
                return parentGraph;
            }
        };
    }

    loadGraph = async () => {
        if (this.graphId !== null) return this.graphId;

        if (this.options.overpassGraph) {
            await ensureOSMGraph(this.options);
        }

        return (this.graphId = loadGraph(this.options.filePath));
    };

    unloadGraph = () => {
        if (this.graphId === null) return false;

        return unloadGraph(this.graphId);
    };

    getNodes = ({ nodes }: { nodes: number[] }) => {
        if (this.graphId === null) throw new Error("Graph is not loaded.");

        return nodes.map((node) => getNode(this.graphId!, node));
    };

    getWays = ({ ways }: { ways: number[] }) => {
        if (this.graphId === null) throw new Error("Graph is not loaded.");

        return ways.map((way) => getWay(this.graphId!, way));
    };

    getShape = ({ nodes }: { nodes: number[] }) => {
        if (this.graphId === null) throw new Error("Graph is not loaded.");

        return getShape(this.graphId!, nodes);
    };
}

const ensureOSMGraph = async ({ filePath, overpassGraph }: GraphOptions) => {
    if (!overpassGraph) return;

    const dir = dirname(filePath);
    if (!existsSync(dir)) mkdirSync(dir, { recursive: true });

    if (
        existsSync(filePath) &&
        statSync(filePath).mtimeMs > Date.now() - overpassGraph.ttlDays * 24 * 60 * 60 * 1000
    ) {
        return;
    }

    const query = `[out:xml][timeout:${overpassGraph.timeout || 1e4}];
        (${overpassGraph.query
            .map(
                (query) =>
                    `${query}(poly: "${overpassGraph.bounds
                        .map(([lon, lat]) => `${lat.toFixed(5)} ${lon.toFixed(5)}`)
                        .join(" ")}");`
            )
            .join("\n")});

        >->.n;
        <->.r;
        (._;.n;.r;);
    out;`;

    let response: string | undefined;

    for (let i = 0; i < (overpassGraph.retries || 3); i++) {
        if (i > 0) {
            await new Promise((resolve) => setTimeout(resolve, overpassGraph.retryDelay || 1000));
        }

        try {
            response = await fetch(
                `https://${overpassGraph.server || "overpass.private.coffee"}/api/interpreter`,
                {
                    method: "POST",
                    body: query,
                }
            ).then((res) => res.text());

            if (response?.includes("Error")) {
                const errorMessage = response.match(/<strong[^>]*>Error<\/strong>:([^<]*)/)?.[1]?.trim();
                throw new Error(decodeURIComponent(errorMessage || "Unknown error"));
            }

            break;
        } catch (e) {
            console.error(`Error fetching OSM data on ${i + 1} attempt:`, e);
        }
    }

    if (!response) throw new Error("Failed to fetch OSM data after retries.");

    writeFileSync(filePath, response);
};

export default Graph;
