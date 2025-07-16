export type Location = [lon: number, lat: number];

export interface RouteResult {
    nodes: number[];
}

export interface OsmNode {
    id: number;
    location: Location;
    tags: Record<string, string>;
}

export interface OsmWay {
    id: number;
    tags: Record<string, string>;
    nodes: OsmNode[];
}

export interface QueueStatus {
    queuedTasks: number;
    activeTasks: number;
    isEmpty: boolean;
}

export type RawProfile = {
    id: string;
    key: string;
    penalties: Record<string, number>;
    access_tags: string[];
    oneway_tags: string[];
    except_tags: string[];
};
