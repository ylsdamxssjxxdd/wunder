package dev.wunder.android;

import android.app.job.JobInfo;
import android.app.job.JobParameters;
import android.app.job.JobScheduler;
import android.app.job.JobService;
import android.content.ComponentName;
import android.content.Context;
import android.util.Log;

/**
 * Periodic cron wake in the dedicated {@code :background} process.
 *
 * <p>JobScheduler is used instead of an alarm-backed foreground service because
 * Android 12+ may refuse a background FGS start from an inexact alarm. The job
 * survives reboots with {@code setPersisted(true)} and needs no exact-alarm
 * grant. Each run starts a headless desktop, drains due jobs and exits; the
 * process separation keeps its thread runtime away from the UI engine.
 */
public final class WunderCronJobService extends JobService {
    private static final String TAG = "wunder-bg";
    private static final int JOB_ID = 4302;
    private static final long INTERVAL_MS = 15L * 60L * 1000L;

    private volatile Thread worker;

    /** (Re)schedule the periodic job. Safe to call on every launch and boot. */
    public static void schedule(Context context) {
        if (context == null) {
            return;
        }
        Context app = context.getApplicationContext();
        if (app == null) {
            app = context;
        }
        JobScheduler scheduler = (JobScheduler) app.getSystemService(Context.JOB_SCHEDULER_SERVICE);
        if (scheduler == null) {
            Log.w(TAG, "job scheduler unavailable");
            return;
        }
        try {
            // Re-scheduling an existing periodic job can reset its next run;
            // frequent launches must not starve the background tick.
            if (scheduler.getPendingJob(JOB_ID) != null) {
                return;
            }
        } catch (Throwable ignored) {
            // Older/OEM schedulers may not expose getPendingJob; schedule
            // below is still idempotent by job id.
        }
        JobInfo job = new JobInfo.Builder(
                JOB_ID,
                new ComponentName(app, WunderCronJobService.class))
                .setPersisted(true)
                .setPeriodic(INTERVAL_MS)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_NONE)
                .build();
        try {
            scheduler.schedule(job);
        } catch (Throwable error) {
            Log.w(TAG, "schedule cron job failed: " + error);
        }
    }

    @Override
    public boolean onStartJob(JobParameters params) {
        if (worker != null && worker.isAlive()) {
            // A previous tick is still running; let it finish and let the
            // scheduler deliver the next period later.
            return false;
        }
        worker = new Thread(() -> {
            try {
                String reply = BackgroundRuntime.tickSafely(getApplicationContext());
                Log.i(TAG, "cron job reply=" + reply);
            } catch (Throwable error) {
                Log.e(TAG, "cron job failed", error);
            } finally {
                jobFinished(params, false);
            }
        }, "wunder-cron-job");
        worker.start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        // The native tick keeps its lease; a stopped job should be retried.
        return true;
    }
}
