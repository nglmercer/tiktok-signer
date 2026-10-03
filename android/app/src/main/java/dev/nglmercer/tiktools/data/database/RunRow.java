package dev.nglmercer.tiktools.data.database;

import androidx.annotation.NonNull;
import androidx.room.*;

@Entity(
    tableName = "runs",
    indices = {
      @Index(
          value = {"at"},
          orders = {Index.Order.DESC},
          name = "idx_runs_at")
    })
public final class RunRow {
  @PrimaryKey(autoGenerate = true)
  @ColumnInfo(name = "id")
  public final long id;

  @ColumnInfo(name = "action_id")
  public final long actionId;

  @ColumnInfo(name = "at")
  public final long at;

  @NonNull
  @ColumnInfo(name = "status")
  public final String status;

  @ColumnInfo(name = "ms", defaultValue = "0")
  public final long ms;

  @NonNull
  @ColumnInfo(name = "detail", defaultValue = "''")
  public final String detail;

  public RunRow(
      long id, long actionId, long at, @NonNull String status, long ms, @NonNull String detail) {
    this.id = id;
    this.actionId = actionId;
    this.at = at;
    this.status = status;
    this.ms = ms;
    this.detail = detail;
  }
}
