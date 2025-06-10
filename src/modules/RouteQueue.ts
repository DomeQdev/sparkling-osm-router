import { cpus } from "os";
import {
    clearRouteQueue,
    createRouteQueue,
    enqueueRoute,
    getQueueStatus,
    startQueueProcessing,
} from "../RustModules";
import { RouteResult } from "../typings";

class RouteQueue {
    queueId: number;
    private enableProgressBar: boolean;
    private processing: boolean = false;

    constructor(
        graphId: number,
        profileId: number,
        enableProgressBar: boolean = false,
        maxConcurrency: number = cpus().length - 1
    ) {
        this.queueId = createRouteQueue(graphId, profileId, maxConcurrency);
        this.enableProgressBar = enableProgressBar;
    }

    enqueueRoute = (routeId: string, startNode: number, endNode: number) => {
        if (this.processing) throw new Error("Queue is already processing. Cannot enqueue new routes.");

        return enqueueRoute(this.queueId, routeId, startNode, endNode);
    };

    getStatus = () => {
        return getQueueStatus(this.queueId);
    };

    clear = () => {
        if (this.processing) throw new Error("Cannot clear queue while processing.");

        return clearRouteQueue(this.queueId);
    };

    awaitAll = async (callback: (id: string, result: RouteResult | null, error?: Error) => void) => {
        if (this.processing) throw new Error("Queue is already processing. Cannot await new routes.");

        const completionTimes: number[] = [];
        const startTime = Date.now();
        let completedTasks = 0;
        let emptyCount = 0;
        this.processing = true;

        return new Promise<void>((resolve) => {
            const checkInterval = setInterval(() => {
                const status = this.getStatus();
                this.updateProgress(completionTimes, startTime, completedTasks, emptyCount);

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
                this.updateProgress(completionTimes, startTime, completedTasks, emptyCount);

                if (result instanceof Error) {
                    callback(id, null, result);
                } else {
                    if (!result || !result.ways.length) emptyCount++;

                    callback(id, result);
                }
            });
        });
    };

    private lastOutputLength: number = 0;

    private updateProgress = (
        completionTimes: number[],
        startTime: number,
        completedTasks: number,
        emptyCount: number
    ) => {
        const status = this.getStatus();
        const remaining = status.queuedTasks + status.activeTasks;
        const total = completedTasks + remaining;
        if (total === 0) return;

        const proportion = completedTasks / total;
        const percent = Math.floor(proportion * 100);

        const currentTime = Date.now();
        const elapsedSeconds = (currentTime - startTime) / 1000;

        const timeWindow = 30 * 1000;
        const cutoffTime = currentTime - timeWindow;

        while (completionTimes.length > 0 && completionTimes[0] < cutoffTime) {
            completionTimes.shift();
        }

        let routesPerSecond = 0;
        if (completionTimes.length > 0 && currentTime - completionTimes[0] > 1000) {
            const windowDuration = (currentTime - completionTimes[0]) / 1000;
            routesPerSecond = completionTimes.length / windowDuration;
        } else {
            routesPerSecond = elapsedSeconds > 0 ? completedTasks / elapsedSeconds : 0;
        }

        let etaString = "";
        if (routesPerSecond > 0 && remaining > 0) {
            const etaSeconds = Math.round(remaining / routesPerSecond);
            const etaMinutes = Math.floor(etaSeconds / 60);
            const etaRemainingSeconds = etaSeconds % 60;
            etaString = `${etaMinutes}m, ${etaRemainingSeconds.toString().padStart(2, "0")}s`;
        }

        const barLength = (process.stdout.columns - 20) / 2;
        const completedLength = Math.floor(barLength * proportion);
        const progressBar = `[${"=".repeat(completedLength) + " ".repeat(barLength - completedLength)}]`;
        const output =
            `${progressBar} ${completedTasks}/${total} (${percent}%)` +
            ` | ${routesPerSecond.toFixed(2)} routes/s` +
            (emptyCount > 0 ? ` | ${emptyCount} empty` : "") +
            (etaString ? ` | ETA: ${etaString}` : "");

        this.lastOutputLength = output.length;

        process.stdout.write(`\r${output + " ".repeat(Math.max(0, this.lastOutputLength - output.length))}`);
    };
}

export default RouteQueue;
