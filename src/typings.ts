export type Location = [number, number];

export interface RouteResult {
    nodes: number[];
    ways: number[];
}

export interface OsmNode {
    id: number;
    lat: number;
    lon: number;
    tags: Record<string, string>;
}

export interface OsmWay {
    id: number;
    nodes: number[];
    tags: Record<string, string>;
}

export interface QueueStatus {
    queuedTasks: number;
    activeTasks: number;
    isEmpty: boolean;
}
