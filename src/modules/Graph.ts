import { loadGraph, unloadGraph } from "../RustModules";
import Profile, { ProfileOptions } from "./Profile";
import { Location, RawProfile } from "../typings";
import { existsSync, mkdirSync } from "fs";
import { dirname } from "path";

export type GraphOptions = {
    filePath: string;
    ttlDays: number;
    overpassGraph: {
        query: string[];
        bounds: Location[];
        server?: string;
        timeout?: number;
        retries?: number;
        retryDelay?: number;
        ignoreTurnRestrictions?: boolean;
    };
};

class Graph {
    public graphId: number | null = null;
    public Profile: typeof Profile;
    private options: GraphOptions;
    private profiles: RawProfile[] = [];

    constructor(options: GraphOptions) {
        const parentGraph = this;
        this.options = options;

        this.Profile = class extends Profile {
            constructor(options: ProfileOptions) {
                super(options);
                this.graph = parentGraph;
                parentGraph.profiles.push(this.rawProfile);
            }
        };
    }

    loadGraph = async () => {
        if (this.graphId !== null) return this.graphId;

        const dir = dirname(this.options.filePath);
        if (!existsSync(dir)) {
            mkdirSync(dir, { recursive: true });
        }

        return (this.graphId = loadGraph(
            JSON.stringify({
                file_path: this.options.filePath,
                ttl_days: this.options.ttlDays,
                profiles: this.profiles,
                overpass: this.overpassConfig,
            })
        ));
    };

    unloadGraph = () => {
        if (this.graphId === null) return false;

        return unloadGraph(this.graphId);
    };

    private get overpassConfig() {
        const overpassOptions = this.options.overpassGraph;
        const bounds = overpassOptions.bounds
            .map(([lon, lat]) => `${lat.toFixed(5)} ${lon.toFixed(5)}`)
            .join(" ");

        const query = `[out:xml][timeout:${overpassOptions.timeout || 1e4}];
        (${overpassOptions.query.map((query) => `${query}(poly: "${bounds}");`).join("\n")});
        ${!overpassOptions.ignoreTurnRestrictions ? ">->.n; <->.r; (._;.n;.r;);" : "(._;>;);"}
        out;`;

        return {
            query,
            server: overpassOptions.server || "https://overpass.private.coffee",
            retries: overpassOptions.retries || 3,
            retry_delay: overpassOptions.retryDelay || 1000,
        };
    }
}

export default Graph;
