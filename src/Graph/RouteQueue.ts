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
     * ID of the starting node
     */
    startNode: number;

    /**
     * ID of the ending node
     */
    endNode: number;

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
    private enableProgressBar: boolean;
    private processing: boolean = false;

    /**
     * Creates a new RouteQueue instance
     * @param graph The graph instance to use for routing
     * @param enableProgressBar Whether to display a progress bar in the console
     * @param maxConcurrency Optional maximum number of concurrent tasks (defaults to CPU count - 1)
     */
    constructor(graph: number, enableProgressBar: boolean, maxConcurrency: number = cpus().length - 1) {
        this.queueId = createRouteQueue(graph, maxConcurrency);
        this.enableProgressBar = enableProgressBar;
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

        return enqueueRoute(this.queueId, id, options.startNode, options.endNode, options.bearing ?? null);
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

        const completionTimes: number[] = [];
        const startTime = Date.now();
        let completedTasks = 0;
        let emptyCount = 0;

        try {
            let lastOutputLength = 0;

            return new Promise<void>((resolve) => {
                const updateProgress = () => {
                    if (!this.enableProgressBar) return;

                    const status = this.getStatus();
                    const completed = completedTasks;
                    const remaining = status.queuedTasks + status.activeTasks;
                    const total = completed + remaining;
                    if (total === 0) return;

                    const percent = Math.floor((completed / total) * 100);

                    const currentTime = Date.now();
                    const elapsedSeconds = (currentTime - startTime) / 1000;

                    const timeWindow = 30 * 1000;
                    const cutoffTime = currentTime - timeWindow;

                    while (completionTimes.length > 0 && completionTimes[0] < cutoffTime) {
                        completionTimes.shift();
                    }

                    let routesPerSecond;
                    if (completionTimes.length > 0 && currentTime - completionTimes[0] > 1000) {
                        const windowDuration = (currentTime - completionTimes[0]) / 1000;
                        routesPerSecond = completionTimes.length / windowDuration;
                    } else {
                        routesPerSecond = elapsedSeconds > 0 ? completed / elapsedSeconds : 0;
                    }

                    let etaString = "";
                    if (routesPerSecond > 0 && remaining > 0) {
                        const etaSeconds = Math.round(remaining / routesPerSecond);
                        const etaMinutes = Math.floor(etaSeconds / 60);
                        const etaRemainingSeconds = etaSeconds % 60;
                        etaString = `${etaMinutes}m, ${etaRemainingSeconds.toString().padStart(2, "0")}s`;
                    }

                    const progressBar = this.getProgressBar(percent);

                    const output =
                        `${progressBar} ${completed}/${total} (${percent}%)` +
                        ` | ${routesPerSecond.toFixed(2)} routes/s` +
                        (emptyCount > 0 ? ` | ${emptyCount} empty` : "") +
                        (etaString ? ` | ETA: ${etaString}` : "");

                    const paddedOutput = output + " ".repeat(Math.max(0, lastOutputLength - output.length));

                    lastOutputLength = output.length;

                    process.stdout.write(`\r${paddedOutput}`);
                };

                const checkInterval = setInterval(() => {
                    const status = this.getStatus();
                    updateProgress();

                    if (status.isEmpty) {
                        if (this.enableProgressBar) {
                            process.stdout.write("\n");
                        }

                        clearInterval(checkInterval);
                        this.processing = false;
                        resolve();
                    }
                }, 750);

                startQueueProcessing(this.queueId, (id, result) => {
                    completedTasks++;

                    completionTimes.push(Date.now());
                    updateProgress();

                    if (result instanceof Error) {
                        callback(id, null, result);
                    } else {
                        if (!result || !result.ways.length) emptyCount++;

                        callback(id, result);
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
