package dev.nglmercer.tiktools.data.database;

import androidx.annotation.NonNull;
import androidx.room.*;

@Entity(
    tableName = "earnings",
    primaryKeys = {"user_id", "category"})
public final class Earning {
  @NonNull
  @ColumnInfo(name = "user_id")
  public final String userId;

  @NonNull
  @ColumnInfo(name = "category")
  public final String category;

  @ColumnInfo(name = "amount")
  public final double amount;

  public Earning(@NonNull String userId, @NonNull String category, double amount) {
    this.userId = userId;
    this.category = category;
    this.amount = amount;
  }
}
