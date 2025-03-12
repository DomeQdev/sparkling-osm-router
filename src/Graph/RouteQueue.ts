import { cpus } from "os";
import {
    clearRouteQueue,
    cleanupRouteQueue,
    createRouteQueue,
    enqueueRoute,
    getQueueStatus,
    QueueStatus,
    startQueueProcessing,
} from "../RustModules";
import { RouteResult } from "./index";

/**
 * Options for queuing a route calculation
 */
export interface RouteQueueOptions {
    /**
     * Array of IDs of possible starting nodes
     */
    startNodes: number[];

    /**
     * Array of IDs of possible ending nodes
     */
    endNodes: number[];

    /**
     * Optional bearing direction in degrees
     */
    bearing?: number;
}

/**
 * RouteQueue allows for efficient batch processing of multiple routing tasks
 * with automatic distribution across processor threads.
 * The queue is processed entirely in Rust for maximum performance.
 */
export class RouteQueue {
    private queueId: number;
    private graph: any;
    private processing: boolean = false;

    /**
     * Creates a new RouteQueue instance
     * @param graph The graph instance to use for routing
     * @param maxConcurrency Optional maximum number of concurrent tasks (defaults to CPU count - 1)
     */
    constructor(graph: any, maxConcurrency: number = cpus().length - 1) {
        this.graph = graph;
        if (!graph.graph) {
            throw new Error("Graph must be loaded before creating a RouteQueue");
        }
        this.queueId = createRouteQueue(graph.graph, maxConcurrency);
    }

    /**
     * Adds a route to the processing queue
     * @param id Unique identifier for this route
     * @param options Route options including start and end nodes
     * @returns The ID of the queued route task
     */
    enqueueRoute(id: string, options: RouteQueueOptions): string {
        if (this.processing) {
            throw new Error("Cannot add routes while queue is being processed");
        }

        return enqueueRoute(this.queueId, id, options.startNodes, options.endNodes, options.bearing ?? null);
    }

    /**
     * Gets the current status of the queue
     * @returns Status information about the queue
     */
    getStatus(): QueueStatus {
        return getQueueStatus(this.queueId);
    }

    /**
     * Clears all queued routes that haven't started processing yet
     */
    clear(): void {
        if (this.processing) {
            throw new Error("Cannot clear queue while it's being processed");
        }

        clearRouteQueue(this.queueId);
    }

    /**
     * Processes all queued route calculations and waits for them to complete
     * @param callback Function called for each completed route with its ID and result
     */
    async awaitAll(callback: (id: string, result: RouteResult | null, error?: Error) => void): Promise<void> {
        if (this.processing) {
            throw new Error("Queue is already being processed");
        }

        this.processing = true;
        const startTime = Date.now();
        let completedTasks = 0;

        try {
            return new Promise<void>((resolve) => {
                const updateProgress = () => {
                    const status = this.getStatus();
                    const completed = completedTasks;
                    const remaining = status.queuedTasks + status.activeTasks;
                    const total = completed + remaining;
                    const percent = total > 0 ? Math.floor((completed / total) * 100) : 0;

                    const elapsedSeconds = (Date.now() - startTime) / 1000;
                    const routesPerSecond =
                        elapsedSeconds > 0 ? (completed / elapsedSeconds).toFixed(2) : "0.00";

                    const progressBar = this.getProgressBar(percent);

                    process.stdout.write(
                        `\r${progressBar} ${completed}/${total} (${percent}%) | ${routesPerSecond} routes/s`
                    );
                };

                const checkInterval = setInterval(() => {
                    const status = this.getStatus();
                    updateProgress();

                    if (status.isEmpty) {
                        clearInterval(checkInterval);
                        this.processing = false;
                        process.stdout.write("\n"); // Add a new line after completion
                        resolve();
                    }
                }, 200);

                startQueueProcessing(this.queueId, (id, result) => {
                    completedTasks++;
                    updateProgress();

                    if (result instanceof Error) {
                        try {
                            callback(id, null, result);
                        } catch (e) {
                            console.error(`Error in route callback for ${id}:`, e);
                        }
                    } else {
                        try {
                            callback(id, result);
                        } catch (e) {
                            console.error(`Error in route callback for ${id}:`, e);
                        }
                    }
                });
            });
        } catch (e) {
            this.processing = false;
            throw e;
        }
    }

    /**
     * Creates a visual progress bar string
     * @param percent Percentage of completion (0-100)
     * @returns Formatted progress bar string
     */
    private getProgressBar(percent: number): string {
        const width = 20;
        const completed = Math.floor((width * percent) / 100);
        const remaining = width - completed;
        return `[${completed > 0 ? "=".repeat(completed) : ""}${remaining > 0 ? " ".repeat(remaining) : ""}]`;
    }

    /**
     * Cleans up resources used by the queue
     */
    cleanup(): void {
        cleanupRouteQueue(this.queueId);
    }
}
