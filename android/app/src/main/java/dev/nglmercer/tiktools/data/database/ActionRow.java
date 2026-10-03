package dev.nglmercer.tiktools.data.database;

import androidx.annotation.NonNull;
import androidx.room.*;

@Entity(tableName = "actions")
public final class ActionRow {
  @PrimaryKey(autoGenerate = true)
  @ColumnInfo(name = "id")
  public final long id;

  @NonNull
  @ColumnInfo(name = "name")
  public final String name;

  @ColumnInfo(name = "enabled", defaultValue = "1")
  public final boolean enabled;

  @NonNull
  @ColumnInfo(name = "triggers")
  public final String triggers;

  @NonNull
  @ColumnInfo(name = "method", defaultValue = "'GET'")
  public final String method;

  @NonNull
  @ColumnInfo(name = "url")
  public final String url;

  @NonNull
  @ColumnInfo(name = "body", defaultValue = "''")
  public final String body;

  @ColumnInfo(name = "cooldown_secs", defaultValue = "0")
  public final long cooldownSecs;

  @NonNull
  @ColumnInfo(name = "gift_name", defaultValue = "''")
  public final String giftName;

  public ActionRow(
      long id,
      @NonNull String name,
      boolean enabled,
      @NonNull String triggers,
      @NonNull String method,
      @NonNull String url,
      @NonNull String body,
      long cooldownSecs,
      @NonNull String giftName) {
    this.id = id;
    this.name = name;
    this.enabled = enabled;
    this.triggers = triggers;
    this.method = method;
    this.url = url;
    this.body = body;
    this.cooldownSecs = cooldownSecs;
    this.giftName = giftName;
  }
}
