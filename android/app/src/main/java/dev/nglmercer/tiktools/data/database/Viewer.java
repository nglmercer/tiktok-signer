package dev.nglmercer.tiktools.data.database;

import androidx.annotation.NonNull;
import androidx.room.*;

@Entity(
    tableName = "viewers",
    indices = {
      @Index(
          value = {"points"},
          orders = {Index.Order.DESC},
          name = "idx_viewers_points")
    })
public final class Viewer {
  @PrimaryKey
  @NonNull
  @ColumnInfo(name = "unique_id")
  public final String uniqueId;

  @NonNull
  @ColumnInfo(name = "nickname", defaultValue = "''")
  public final String nickname;

  @ColumnInfo(name = "points", defaultValue = "0")
  public final double points;

  @ColumnInfo(name = "level", defaultValue = "1")
  public final int level;

  @ColumnInfo(name = "updated_at", defaultValue = "0")
  public final long updatedAt;

  public Viewer(
      @NonNull String uniqueId,
      @NonNull String nickname,
      double points,
      int level,
      long updatedAt) {
    this.uniqueId = uniqueId;
    this.nickname = nickname;
    this.points = points;
    this.level = level;
    this.updatedAt = updatedAt;
  }
}
