package dev.nglmercer.tiktools.data.database;

import androidx.room.*;
import java.util.List;
import kotlinx.coroutines.flow.Flow;

@Dao
public interface ViewerDao {
  @Query("SELECT * FROM viewers ORDER BY points DESC LIMIT 100")
  Flow<List<Viewer>> observeLeaderboard();

  @Query("SELECT COUNT(*) FROM viewers")
  Flow<Integer> observeCount();

  @Query("SELECT * FROM viewers WHERE unique_id=:id")
  Viewer find(String id);

  @Insert(onConflict = OnConflictStrategy.REPLACE)
  void upsert(Viewer viewer);

  @Query("UPDATE viewers SET points=0, level=1 WHERE :id IS NULL OR unique_id=:id")
  void reset(String id);

  @Query("SELECT * FROM earnings WHERE user_id=:id ORDER BY amount DESC")
  Flow<List<Earning>> observeEarnings(String id);

  @Query("SELECT * FROM earnings WHERE user_id=:id AND category=:category")
  Earning earning(String id, String category);

  @Insert(onConflict = OnConflictStrategy.REPLACE)
  void upsertEarning(Earning earning);

  @Query("DELETE FROM earnings WHERE :id IS NULL OR user_id=:id")
  void resetEarnings(String id);
}
