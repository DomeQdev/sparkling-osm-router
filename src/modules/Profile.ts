import {
    getNearestNode,
    getNode,
    getNodesInRadius,
    getRoute,
    getShape,
    getWaysInRadius,
} from "../RustModules";
import { Location, OsmNode, OsmWay, RawProfile, RouteResult } from "../typings";
import Graph from "./Graph";
import RouteQueue from "./RouteQueue";

type HighwayValue =
    | "motorway"
    | "motorway_link"
    | "trunk"
    | "trunk_link"
    | "primary"
    | "primary_link"
    | "secondary"
    | "secondary_link"
    | "tertiary"
    | "tertiary_link"
    | "unclassified"
    | "residential"
    | "service"
    | "living_street"
    | "pedestrian"
    | "track"
    | "path"
    | "footway"
    | "cycleway"
    | "bridleway"
    | "steps"
    | "corridor"
    | "elevator"
    | "default";

type RailwayValue =
    | "rail"
    | "light_rail"
    | "subway"
    | "tram"
    | "monorail"
    | "narrow_gauge"
    | "funicular"
    | "preserved"
    | "miniature"
    | "default";

export type ProfileOptions = (
    | {
          key: "highway";
          penalties: [HighwayValue | HighwayValue[], number][];
      }
    | {
          key: "railway";
          penalties: [RailwayValue | RailwayValue[], number][];
      }
) & {
    id: string;
    accessTags?: string[];
    onewayTags?: string[];
    exceptTags?: string[];
};

class Profile {
    public rawProfile: RawProfile;
    public graph: Graph;

    constructor(profile: ProfileOptions) {
        const convertedPenalties: Record<string, number> = {};

        for (const [key, value] of profile.penalties) {
            if (Array.isArray(key)) {
                for (const k of key) {
                    convertedPenalties[k] = value;
                }
            } else {
                convertedPenalties[key] = value;
            }
        }

        this.rawProfile = {
            id: profile.id,
            key: profile.key,
            penalties: convertedPenalties,
            access_tags: Array.from(new Set([...(profile.accessTags ?? []), "access"])),
            oneway_tags: Array.from(new Set([...(profile.onewayTags ?? []), "oneway"])),
            except_tags: profile.exceptTags ?? [],
        };
    }

    getNearestNode = ([lon, lat]: Location): number | null => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return getNearestNode(this.graph.graphId, this.rawProfile.id, lon, lat);
    };

    getNodesInRadius = ([lon, lat]: Location, radiusMeters: number): OsmNode[] => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return getNodesInRadius(this.graph.graphId, this.rawProfile.id, lon, lat, radiusMeters);
    };

    getWaysInRadius = ([lon, lat]: Location, radiusMeters: number): OsmWay[] => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return getWaysInRadius(this.graph.graphId, this.rawProfile.id, lon, lat, radiusMeters);
    };

    getRoute = async (waypoints: number[]) => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return getRoute(this.graph.graphId, this.rawProfile.id, waypoints);
    };

    getNode = (node: number): OsmNode | null => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return getNode(this.graph.graphId, this.rawProfile.id, node);
    };

    getShape = ({ nodes }: RouteResult): Location[] => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return getShape(this.graph.graphId, this.rawProfile.id, nodes);
    };

    createRouteQueue = (enableProgressBar?: boolean, maxConcurrency?: number) => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return new RouteQueue(this.graph.graphId, this.rawProfile.id, enableProgressBar, maxConcurrency);
    };
}

export default Profile;
