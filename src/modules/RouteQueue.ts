import { RouteResult } from "../typings";
import cliProgress from "cli-progress";
import { cpus } from "os";
import {
    clearRouteQueue,
    createRouteQueue,
    enqueueRoute,
    getQueueStatus,
    processQueue,
} from "../RustModules";

class RouteQueue {
    public queueId: number;
    private enableProgressBar: boolean;
    private processing: boolean = false;

    constructor(
        graphId: number,
        profileId: string,
        enableProgressBar: boolean = false,
        maxConcurrency: number = cpus().length - 1
    ) {
        this.queueId = createRouteQueue(graphId, profileId, maxConcurrency);
        this.enableProgressBar = enableProgressBar;
    }

    enqueueRoute = (routeId: string, waypoints: number[]) => {
        if (this.processing) throw new Error("Queue is already processing. Cannot enqueue new routes.");
        return enqueueRoute(this.queueId, routeId, waypoints);
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

        const initialStatus = this.getStatus();
        const totalTasks = initialStatus.queuedTasks;

        if (totalTasks === 0) {
            return Promise.resolve();
        }

        this.processing = true;

        return new Promise<void>((resolve) => {
            const progressHistory: { time: number; tasks: number }[] = [];
            let completedTasks = 0;
            let emptyCount = 0;
            let bar: cliProgress.SingleBar | null = null;

            if (this.enableProgressBar) {
                bar = new cliProgress.SingleBar(
                    {
                        format: "Processing |{bar}| {percentage}% | {value}/{total} | ETA: {eta} | Speed: {speed} routes/s | Empty: {emptyCount}",
                        stopOnComplete: true,
                    },
                    cliProgress.Presets.shades_classic
                );
                bar.start(totalTasks, 0, { speed: "N/A", eta: "N/A", emptyCount: 0 });
            }

            processQueue(this.queueId, (id, result) => {
                if (result instanceof Error) {
                    callback(id, null, result);
                } else {
                    if (!result || !result.nodes || !result.nodes.length) emptyCount++;
                    callback(id, result);
                }

                completedTasks++;

                if (bar) {
                    const now = Date.now();
                    progressHistory.push({ time: now, tasks: completedTasks });

                    while (progressHistory.length > 0 && now - progressHistory[0].time > 30 * 1000) {
                        progressHistory.shift();
                    }

                    let speed: string | number = "N/A";
                    let eta = "N/A";

                    if (progressHistory.length > 1) {
                        const first = progressHistory[0];
                        const last = progressHistory[progressHistory.length - 1];
                        const timeDiffSeconds = (last.time - first.time) / 1000;
                        const tasksDiff = last.tasks - first.tasks;

                        if (timeDiffSeconds > 0) {
                            const currentSpeed = tasksDiff / timeDiffSeconds;
                            speed = Math.round(currentSpeed);

                            const remainingTasks = totalTasks - completedTasks;
                            if (currentSpeed > 0) {
                                const etaSeconds = Math.round(remainingTasks / currentSpeed);
                                const h = Math.floor(etaSeconds / 3600);
                                const m = Math.floor((etaSeconds % 3600) / 60);
                                const s = Math.floor(etaSeconds % 60);
                                eta = `${h > 0 ? h + "h " : ""}${m > 0 ? m + "m " : ""}${s}s`;
                            }
                        }
                    }
                    
                    bar.increment(1, { speed, eta, emptyCount });
                }

                if (completedTasks >= totalTasks) {
                    if (bar) bar.stop();
                    this.processing = false;
                    resolve();
                }
            });
        });
    };
}

export default RouteQueue;
