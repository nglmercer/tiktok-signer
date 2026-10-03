package dev.nglmercer.tiktools.data.database;

import androidx.room.*;

@Database(
    entities = {Viewer.class, ActionRow.class, RunRow.class, Earning.class},
    version = 2,
    exportSchema = true)
public abstract class TikToolsDatabase extends RoomDatabase {
  public abstract ViewerDao viewers();

  public abstract AutomationDao automations();

  public abstract ActionRunDao runs();
}
